//! Core Audio process-tap capture for the synthetic "System Audio" source.
//!
//! The tap feeds the same 48 kHz mono capture contract as the CPAL microphone
//! backend. The CoreAudio callback must stay realtime-safe: no blocking, no
//! heap allocation after startup, and bounded lossy handoff into the graph.

use std::ffi::c_void;
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use intervox_core::audio::level_meter::LevelMeter;
use intervox_core::audio::resampler::LinearResampler;
use intervox_core::AppError;
use objc2_core_audio::{
    kAudioAggregateDeviceIsPrivateKey, kAudioAggregateDeviceNameKey,
    kAudioAggregateDeviceTapListKey, kAudioAggregateDeviceUIDKey,
    kAudioHardwarePropertyTranslatePIDToProcessObject, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kAudioObjectUnknown,
    kAudioSubTapUIDKey, kAudioTapPropertyFormat, kAudioTapPropertyUID, AudioDeviceCreateIOProcID,
    AudioDeviceDestroyIOProcID, AudioDeviceIOProcID, AudioDeviceStart, AudioDeviceStop,
    AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap,
    AudioHardwareDestroyAggregateDevice, AudioHardwareDestroyProcessTap,
    AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress, CATapDescription,
    CATapMuteBehavior,
};
use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsNonInterleaved, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM, AudioBuffer, AudioBufferList, AudioStreamBasicDescription,
    AudioTimeStamp,
};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_foundation::{NSArray, NSNumber, NSString};

use super::capture::{
    CaptureHandle, CapturedFrame, MAX_CALLBACK_FRAMES, MAX_CAPTURE_OUTPUT_FRAMES, SINK_BOUND,
    TARGET_HZ,
};
use super::AudioBackpressureCounters;

const SYSTEM_AUDIO_THREAD: &str = "system-audio-capture";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_INDEX: isize = 0;
type OSStatus = i32;

#[derive(Debug, Clone, Copy)]
enum SampleKind {
    F32,
    F64,
    I16,
    I32,
}

impl SampleKind {
    fn bytes(self) -> usize {
        match self {
            Self::F32 | Self::I32 => 4,
            Self::F64 => 8,
            Self::I16 => 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TapFormat {
    sample_rate: u32,
    channels: u32,
    non_interleaved: bool,
    sample: SampleKind,
}

impl TapFormat {
    fn from_asbd(asbd: AudioStreamBasicDescription) -> Result<Self, AppError> {
        if asbd.mFormatID != kAudioFormatLinearPCM {
            return Err(AppError::audio_device_unavailable(format!(
                "System audio tap produced unsupported format id {}.",
                asbd.mFormatID
            )));
        }

        let sample = if asbd.mFormatFlags & kAudioFormatFlagIsFloat != 0 {
            match asbd.mBitsPerChannel {
                32 => SampleKind::F32,
                64 => SampleKind::F64,
                bits => {
                    return Err(AppError::audio_device_unavailable(format!(
                        "System audio tap produced unsupported float sample width {bits}."
                    )))
                }
            }
        } else if asbd.mFormatFlags & kAudioFormatFlagIsSignedInteger != 0 {
            match asbd.mBitsPerChannel {
                16 => SampleKind::I16,
                32 => SampleKind::I32,
                bits => {
                    return Err(AppError::audio_device_unavailable(format!(
                        "System audio tap produced unsupported integer sample width {bits}."
                    )))
                }
            }
        } else {
            return Err(AppError::audio_device_unavailable(
                "System audio tap produced unsupported PCM sample encoding.",
            ));
        };

        if asbd.mSampleRate <= 0.0 || asbd.mChannelsPerFrame == 0 {
            return Err(AppError::audio_device_unavailable(
                "System audio tap produced an invalid stream format.",
            ));
        }

        Ok(Self {
            sample_rate: asbd.mSampleRate.round() as u32,
            channels: asbd.mChannelsPerFrame,
            non_interleaved: asbd.mFormatFlags & kAudioFormatFlagIsNonInterleaved != 0,
            sample,
        })
    }
}

struct CallbackState {
    sink: SyncSender<CapturedFrame>,
    pool_tx: SyncSender<Vec<f32>>,
    pool_rx: Receiver<Vec<f32>>,
    level: Arc<AtomicU32>,
    level_sequence: Arc<AtomicU64>,
    backpressure: Arc<AudioBackpressureCounters>,
    format: TapFormat,
    resampler: LinearResampler,
    mono: Vec<f32>,
}

impl CallbackState {
    fn new(
        sink: SyncSender<CapturedFrame>,
        pool_tx: SyncSender<Vec<f32>>,
        pool_rx: Receiver<Vec<f32>>,
        level: Arc<AtomicU32>,
        level_sequence: Arc<AtomicU64>,
        backpressure: Arc<AudioBackpressureCounters>,
        format: TapFormat,
    ) -> Self {
        let mut resampler = LinearResampler::new(format.sample_rate, TARGET_HZ);
        resampler.reserve_for(MAX_CALLBACK_FRAMES);
        Self {
            sink,
            pool_tx,
            pool_rx,
            level,
            level_sequence,
            backpressure,
            format,
            resampler,
            mono: Vec::with_capacity(MAX_CALLBACK_FRAMES),
        }
    }

    fn process_input(&mut self, input: NonNull<AudioBufferList>) {
        let ok = unsafe { audio_buffer_list_to_mono(input, self.format, &mut self.mono) };
        if !ok || self.mono.is_empty() || self.mono.len() > MAX_CALLBACK_FRAMES {
            self.backpressure.capture_capacity_drop();
            return;
        }

        let level = LevelMeter::measure(&self.mono);
        self.level.store(level.rms.to_bits(), Ordering::Relaxed);
        self.level_sequence.fetch_add(1, Ordering::Release);

        let Ok(mut resampled) = self.pool_rx.try_recv() else {
            self.backpressure.capture_pool_miss();
            return;
        };
        if self.resampler.max_output_len(self.mono.len()) > resampled.capacity() {
            let _ = self.pool_tx.try_send(resampled);
            self.backpressure.capture_capacity_drop();
            return;
        }

        self.resampler.process_into(&self.mono, &mut resampled);
        if self
            .sink
            .try_send(CapturedFrame::new(resampled, self.pool_tx.clone()))
            .is_err()
        {
            self.backpressure.capture_sink_drop();
        }
    }
}

pub fn start(
    level: Arc<AtomicU32>,
    level_sequence: Arc<AtomicU64>,
    backpressure: Arc<AudioBackpressureCounters>,
) -> Result<(CaptureHandle, std::sync::mpsc::Receiver<CapturedFrame>), AppError> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<CapturedFrame>(SINK_BOUND);
    let (pool_tx, pool_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(SINK_BOUND);
    for _ in 0..SINK_BOUND {
        pool_tx
            .try_send(Vec::with_capacity(MAX_CAPTURE_OUTPUT_FRAMES))
            .map_err(|_| AppError::internal("system audio buffer pool initialization failed"))?;
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), AppError>>(1);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let thread = std::thread::Builder::new()
        .name(SYSTEM_AUDIO_THREAD.to_string())
        .spawn(move || {
            let mut resources = match TapResources::create() {
                Ok(resources) => resources,
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };

            let format = match resources.tap_format() {
                Ok(format) => format,
                Err(error) => {
                    resources.destroy();
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };

            let mut state = CallbackState::new(
                tx,
                pool_tx,
                pool_rx,
                level,
                level_sequence,
                backpressure,
                format,
            );

            if let Err(error) = resources.start(&mut state) {
                resources.destroy();
                let _ = ready_tx.send(Err(error));
                return;
            }

            let _ = ready_tx.send(Ok(()));

            while !stop_thread.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(50));
            }

            resources.stop_and_destroy();
        })
        .map_err(|e| AppError::internal(format!("spawn system audio capture thread: {e}")))?;

    match ready_rx.recv_timeout(STARTUP_TIMEOUT) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = thread.join();
            return Err(e);
        }
        Err(RecvTimeoutError::Timeout) => {
            stop.store(true, Ordering::Release);
            thread.thread().unpark();
            return Err(AppError::internal(format!(
                "system audio capture startup timed out after {}s",
                STARTUP_TIMEOUT.as_secs()
            )));
        }
        Err(RecvTimeoutError::Disconnected) => {
            let _ = thread.join();
            return Err(AppError::internal(
                "system audio capture thread exited before startup",
            ));
        }
    }

    Ok((CaptureHandle::new(stop, thread), rx))
}

struct TapResources {
    tap_id: AudioObjectID,
    aggregate_id: AudioObjectID,
    io_proc_id: AudioDeviceIOProcID,
    started: bool,
}

impl TapResources {
    fn create() -> Result<Self, AppError> {
        let default_output = crate::devices::default_output_device_id().ok_or_else(|| {
            AppError::audio_device_unavailable("No macOS output device is available.")
        })?;
        let default_output_uid =
            crate::devices::uid_from_device_id(&default_output).ok_or_else(|| {
                AppError::audio_device_unavailable("The default output device id is invalid.")
            })?;
        let current_process = current_process_object_id()?;

        let excluded_process = NSNumber::new_u32(current_process);
        let excluded = NSArray::from_retained_slice(&[excluded_process]);
        let description = unsafe { CATapDescription::new() };
        let name = NSString::from_str("Intervox System Audio");
        let output_uid = NSString::from_str(default_output_uid);
        let stream = NSNumber::new_isize(STREAM_INDEX);
        unsafe {
            description.setName(&name);
            description.setPrivate(true);
            description.setMuteBehavior(CATapMuteBehavior::Unmuted);
            description.setProcesses(&excluded);
            description.setExclusive(true);
            description.setMixdown(true);
            description.setMono(true);
            description.setDeviceUID(Some(&output_uid));
            description.setStream(Some(&stream));
        }

        let mut tap_id = kAudioObjectUnknown;
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut tap_id) };
        if status != 0 || tap_id == kAudioObjectUnknown {
            return Err(system_audio_start_error(
                "AudioHardwareCreateProcessTap",
                status,
            ));
        }

        let tap_uid = match tap_uid(tap_id) {
            Ok(uid) => uid,
            Err(error) => {
                unsafe {
                    AudioHardwareDestroyProcessTap(tap_id);
                }
                return Err(error);
            }
        };

        let aggregate_description = aggregate_description(&tap_uid);
        let mut aggregate_id = kAudioObjectUnknown;
        let status = unsafe {
            AudioHardwareCreateAggregateDevice(
                aggregate_description.as_ref(),
                NonNull::new((&mut aggregate_id as *mut AudioObjectID).cast()).unwrap(),
            )
        };
        if status != 0 || aggregate_id == kAudioObjectUnknown {
            unsafe {
                AudioHardwareDestroyProcessTap(tap_id);
            }
            return Err(system_audio_start_error(
                "AudioHardwareCreateAggregateDevice",
                status,
            ));
        }

        Ok(Self {
            tap_id,
            aggregate_id,
            io_proc_id: None,
            started: false,
        })
    }

    fn tap_format(&self) -> Result<TapFormat, AppError> {
        TapFormat::from_asbd(tap_format(self.tap_id)?)
    }

    fn start(&mut self, state: &mut CallbackState) -> Result<(), AppError> {
        let mut io_proc_id: AudioDeviceIOProcID = None;
        let status = unsafe {
            AudioDeviceCreateIOProcID(
                self.aggregate_id,
                Some(system_audio_ioproc),
                (state as *mut CallbackState).cast::<c_void>(),
                NonNull::new((&mut io_proc_id as *mut AudioDeviceIOProcID).cast()).unwrap(),
            )
        };
        if status != 0 {
            return Err(system_audio_start_error(
                "AudioDeviceCreateIOProcID",
                status,
            ));
        }
        if io_proc_id.is_none() {
            return Err(AppError::internal(
                "CoreAudio returned a null system audio IOProc ID",
            ));
        }
        self.io_proc_id = io_proc_id;

        let status = unsafe { AudioDeviceStart(self.aggregate_id, io_proc_id) };
        if status != 0 {
            unsafe {
                AudioDeviceDestroyIOProcID(self.aggregate_id, io_proc_id);
            }
            self.io_proc_id = None;
            return Err(system_audio_start_error("AudioDeviceStart", status));
        }
        self.started = true;
        Ok(())
    }

    fn stop_and_destroy(&mut self) {
        if let Some(io_proc_id) = self.io_proc_id.take() {
            let io_proc_id = Some(io_proc_id);
            if self.started {
                unsafe {
                    AudioDeviceStop(self.aggregate_id, io_proc_id);
                }
            }
            unsafe {
                AudioDeviceDestroyIOProcID(self.aggregate_id, io_proc_id);
            }
        }
        self.started = false;
        self.destroy();
    }

    fn destroy(&mut self) {
        if self.aggregate_id != kAudioObjectUnknown {
            unsafe {
                AudioHardwareDestroyAggregateDevice(self.aggregate_id);
            }
            self.aggregate_id = kAudioObjectUnknown;
        }
        if self.tap_id != kAudioObjectUnknown {
            unsafe {
                AudioHardwareDestroyProcessTap(self.tap_id);
            }
            self.tap_id = kAudioObjectUnknown;
        }
    }
}

impl Drop for TapResources {
    fn drop(&mut self) {
        self.stop_and_destroy();
    }
}

fn aggregate_description(tap_uid: &CFString) -> CFRetained<CFDictionary<CFType, CFType>> {
    let aggregate_uid =
        CFString::from_str(&format!("app.intervox.system-audio.{}", std::process::id()));
    let aggregate_name = CFString::from_str("Intervox System Audio Capture");
    let key_uid = CFString::from_str(kAudioAggregateDeviceUIDKey.to_str().unwrap_or("uid"));
    let key_name = CFString::from_str(kAudioAggregateDeviceNameKey.to_str().unwrap_or("name"));
    let key_private = CFString::from_str(
        kAudioAggregateDeviceIsPrivateKey
            .to_str()
            .unwrap_or("private"),
    );
    let key_taps = CFString::from_str(kAudioAggregateDeviceTapListKey.to_str().unwrap_or("taps"));
    let sub_tap_uid_key = CFString::from_str(kAudioSubTapUIDKey.to_str().unwrap_or("uid"));
    let private = CFBoolean::new(true);
    let sub_tap = CFDictionary::<CFType, CFType>::from_slices(
        &[sub_tap_uid_key.as_ref()],
        &[tap_uid.as_ref()],
    );
    let taps = CFArray::<CFDictionary<CFType, CFType>>::from_objects(&[sub_tap.as_ref()]);

    CFDictionary::<CFType, CFType>::from_slices(
        &[
            key_uid.as_ref(),
            key_name.as_ref(),
            key_private.as_ref(),
            key_taps.as_ref(),
        ],
        &[
            aggregate_uid.as_ref(),
            aggregate_name.as_ref(),
            private.as_ref(),
            taps.as_ref(),
        ],
    )
}

fn current_process_object_id() -> Result<AudioObjectID, AppError> {
    let mut address = property_address(kAudioHardwarePropertyTranslatePIDToProcessObject);
    let pid = std::process::id() as libc::pid_t;
    let mut process_id = kAudioObjectUnknown;
    let mut size = std::mem::size_of::<AudioObjectID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&mut address),
            std::mem::size_of::<libc::pid_t>() as u32,
            (&pid as *const libc::pid_t).cast::<c_void>(),
            NonNull::from(&mut size),
            NonNull::new((&mut process_id as *mut AudioObjectID).cast::<c_void>()).unwrap(),
        )
    };
    if status != 0 || process_id == kAudioObjectUnknown {
        return Err(AppError::audio_device_unavailable(format!(
            "Intervox cannot safely exclude its own audio from system capture (OSStatus {status})."
        )));
    }
    Ok(process_id)
}

fn tap_uid(tap_id: AudioObjectID) -> Result<CFRetained<CFString>, AppError> {
    let mut address = property_address(kAudioTapPropertyUID);
    let mut raw_uid: *const CFString = ptr::null();
    let mut size = std::mem::size_of::<*const CFString>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            tap_id,
            NonNull::from(&mut address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::new((&mut raw_uid as *mut *const CFString).cast::<c_void>()).unwrap(),
        )
    };
    if status != 0 || raw_uid.is_null() {
        return Err(AppError::audio_device_unavailable(format!(
            "System audio tap did not expose a tap UID (OSStatus {status})."
        )));
    }
    let raw_uid = NonNull::new(raw_uid.cast_mut()).ok_or_else(|| {
        AppError::audio_device_unavailable("System audio tap UID was null after validation.")
    })?;
    Ok(unsafe { CFRetained::from_raw(raw_uid) })
}

fn tap_format(tap_id: AudioObjectID) -> Result<AudioStreamBasicDescription, AppError> {
    let mut address = property_address(kAudioTapPropertyFormat);
    let mut format = std::mem::MaybeUninit::<AudioStreamBasicDescription>::zeroed();
    let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            tap_id,
            NonNull::from(&mut address),
            0,
            ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(format.as_mut_ptr().cast::<c_void>()).unwrap(),
        )
    };
    if status != 0 {
        return Err(AppError::audio_device_unavailable(format!(
            "System audio tap format is unavailable (OSStatus {status})."
        )));
    }
    Ok(unsafe { format.assume_init() })
}

fn property_address(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    }
}

fn system_audio_start_error(operation: &'static str, status: OSStatus) -> AppError {
    AppError::system_audio_permission_denied(format!(
        "Intervox could not start system audio capture ({operation} returned OSStatus {status}). Allow Intervox in System Settings > Privacy & Security > Screen & System Audio Recording, then try again."
    ))
}

unsafe extern "C-unwind" fn system_audio_ioproc(
    _in_device: AudioObjectID,
    _in_now: NonNull<AudioTimeStamp>,
    in_input_data: NonNull<AudioBufferList>,
    _in_input_time: NonNull<AudioTimeStamp>,
    _out_output_data: NonNull<AudioBufferList>,
    _in_output_time: NonNull<AudioTimeStamp>,
    in_client_data: *mut c_void,
) -> OSStatus {
    let Some(state) = in_client_data.cast::<CallbackState>().as_mut() else {
        return 0;
    };
    state.process_input(in_input_data);
    0
}

unsafe fn audio_buffer_list_to_mono(
    list: NonNull<AudioBufferList>,
    format: TapFormat,
    out: &mut Vec<f32>,
) -> bool {
    out.clear();
    let buffers = audio_buffers(list);
    if buffers.is_empty() {
        return false;
    }

    if format.non_interleaved && buffers.len() > 1 {
        let frames = buffers
            .iter()
            .filter_map(|buffer| sample_count(*buffer, format.sample))
            .min()
            .unwrap_or(0);
        if frames == 0 || frames > out.capacity() {
            return false;
        }
        for frame in 0..frames {
            let mut sum = 0.0;
            let mut count = 0usize;
            for buffer in buffers {
                if buffer.mData.is_null() {
                    continue;
                }
                sum += read_sample(buffer.mData.cast::<u8>(), frame, format.sample);
                count += 1;
            }
            if count == 0 {
                return false;
            }
            out.push(sum / count as f32);
        }
        return true;
    }

    for buffer in buffers {
        if buffer.mData.is_null() {
            continue;
        }
        let Some(samples) = sample_count(*buffer, format.sample) else {
            return false;
        };
        let channels = buffer.mNumberChannels.max(format.channels).max(1) as usize;
        let frames = samples / channels;
        if frames == 0 || out.len().saturating_add(frames) > out.capacity() {
            return false;
        }
        for frame in 0..frames {
            let mut sum = 0.0;
            for channel in 0..channels {
                sum += read_sample(
                    buffer.mData.cast::<u8>(),
                    frame * channels + channel,
                    format.sample,
                );
            }
            out.push(sum / channels as f32);
        }
    }
    true
}

unsafe fn audio_buffers<'a>(list: NonNull<AudioBufferList>) -> &'a [AudioBuffer] {
    let list = list.as_ref();
    std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize)
}

fn sample_count(buffer: AudioBuffer, sample: SampleKind) -> Option<usize> {
    let bytes = sample.bytes();
    let data_bytes = buffer.mDataByteSize as usize;
    data_bytes.is_multiple_of(bytes).then_some(data_bytes / bytes)
}

unsafe fn read_sample(data: *const u8, index: usize, kind: SampleKind) -> f32 {
    match kind {
        SampleKind::F32 => data.cast::<f32>().add(index).read_unaligned(),
        SampleKind::F64 => data.cast::<f64>().add(index).read_unaligned() as f32,
        SampleKind::I16 => data.cast::<i16>().add(index).read_unaligned() as f32 / i16::MAX as f32,
        SampleKind::I32 => data.cast::<i32>().add(index).read_unaligned() as f32 / i32::MAX as f32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_format_accepts_float32_pcm() {
        let format = TapFormat::from_asbd(AudioStreamBasicDescription {
            mSampleRate: 48_000.0,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 1,
            mBitsPerChannel: 32,
            mReserved: 0,
        })
        .expect("float32 pcm should be supported");
        assert_eq!(format.sample_rate, 48_000);
        assert!(matches!(format.sample, SampleKind::F32));
    }

    #[test]
    fn tap_format_rejects_unknown_encoding() {
        let error = TapFormat::from_asbd(AudioStreamBasicDescription {
            mSampleRate: 48_000.0,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: 0,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 1,
            mBitsPerChannel: 32,
            mReserved: 0,
        })
        .expect_err("unsigned pcm must be rejected");
        assert!(error.message.contains("unsupported PCM sample encoding"));
    }
}
