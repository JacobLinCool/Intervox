//! Real audio device enumeration via CoreAudio property queries.
//!
//! First principle: listing devices must not instantiate AudioUnits, create
//! IOProc IDs, or ask a driver for stream formats. Those operations can block
//! in coreaudiod and were the root cause of slow startup/high daemon CPU.
//! Enumeration here only reads the system device list, default device IDs,
//! device names, and per-scope stream presence.

use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;

use crate::commands::{AudioDevices, DeviceInfo};

type AudioObjectID = u32;
type AudioDeviceID = u32;
type AudioObjectPropertySelector = u32;
type AudioObjectPropertyScope = u32;
type AudioObjectPropertyElement = u32;
type OSStatus = i32;

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
    element: AudioObjectPropertyElement,
}

const AUDIO_NO_ERROR: OSStatus = 0;
const SYSTEM_OBJECT: AudioObjectID = 1;
const ELEMENT_MASTER: AudioObjectPropertyElement = 0;

const SCOPE_GLOBAL: AudioObjectPropertyScope = 0x676c_6f62; // 'glob'
const SCOPE_INPUT: AudioObjectPropertyScope = 0x696e_7074; // 'inpt'
const SCOPE_OUTPUT: AudioObjectPropertyScope = 0x6f75_7470; // 'outp'

const PROP_DEVICES: AudioObjectPropertySelector = 0x6465_7623; // 'dev#'
const PROP_DEFAULT_INPUT: AudioObjectPropertySelector = 0x6449_6e20; // 'dIn '
const PROP_DEFAULT_OUTPUT: AudioObjectPropertySelector = 0x644f_7574; // 'dOut'
const PROP_DEVICE_NAME_CFSTRING: AudioObjectPropertySelector = 0x6c6e_616d; // 'lnam'
const PROP_DEVICE_UID: AudioObjectPropertySelector = 0x7569_6420; // 'uid '
const PROP_STREAMS: AudioObjectPropertySelector = 0x7374_6d23; // 'stm#'

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
pub const CORE_AUDIO_UID_PREFIX: &str = "coreaudio:uid:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInputDevice {
    pub uid: String,
    pub name: String,
    pub duplicate_name_count: usize,
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyDataSize(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        out_data_size: *mut u32,
    ) -> OSStatus;

    fn AudioObjectGetPropertyData(
        object_id: AudioObjectID,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        io_data_size: *mut u32,
        out_data: *mut c_void,
    ) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(
        string: *const c_void,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
}

/// Enumerate all available input and output audio devices. The system default
/// device appears first in each list; subsequent devices are keyed by CoreAudio
/// UID, not display name. Display names are labels; UIDs are the identity that
/// can survive hotplug and same-name devices.
pub fn enumerate() -> AudioDevices {
    let devices = device_ids();

    let inputs = collect_devices(&devices, default_device(PROP_DEFAULT_INPUT), SCOPE_INPUT);
    let outputs = collect_devices(&devices, default_device(PROP_DEFAULT_OUTPUT), SCOPE_OUTPUT);

    AudioDevices { inputs, outputs }
}

fn collect_devices(
    all: &[AudioDeviceID],
    default: Option<AudioDeviceID>,
    scope: AudioObjectPropertyScope,
) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    if let Some(id) = default {
        push_device(&mut out, &mut seen, id, scope);
    }

    for &id in all {
        if Some(id) != default {
            push_device(&mut out, &mut seen, id, scope);
        }
    }

    out
}

fn push_device(
    out: &mut Vec<DeviceInfo>,
    seen: &mut HashSet<String>,
    id: AudioDeviceID,
    scope: AudioObjectPropertyScope,
) {
    if !has_streams(id, scope) {
        return;
    }
    let Some(name) = device_name(id) else {
        return;
    };
    let Some(uid) = device_uid(id) else {
        return;
    };
    if seen.insert(uid.clone()) {
        out.push(DeviceInfo {
            id: device_id_from_uid(&uid),
            name,
        });
    }
}

pub fn is_coreaudio_uid_id(device_id: &str) -> bool {
    device_id.starts_with(CORE_AUDIO_UID_PREFIX)
}

pub fn device_id_from_uid(uid: &str) -> String {
    format!("{CORE_AUDIO_UID_PREFIX}{uid}")
}

pub fn uid_from_device_id(device_id: &str) -> Option<&str> {
    device_id.strip_prefix(CORE_AUDIO_UID_PREFIX)
}

pub fn input_device_name_for_id(device_id: &str) -> Option<String> {
    resolve_input_device_id(device_id).map(|device| device.name)
}

pub fn resolve_input_device_id(device_id: &str) -> Option<ResolvedInputDevice> {
    let target_uid = uid_from_device_id(device_id)?;
    let devices = device_ids();
    let mut selected = None;
    let mut input_names = Vec::new();

    for id in devices {
        if !has_streams(id, SCOPE_INPUT) {
            continue;
        }
        let Some(uid) = device_uid(id) else {
            continue;
        };
        let Some(name) = device_name(id) else {
            continue;
        };
        if uid == target_uid {
            selected = Some((uid, name.clone()));
        }
        input_names.push(name);
    }

    let (uid, name) = selected?;
    let duplicate_name_count = input_names
        .iter()
        .filter(|candidate| candidate.as_str() == name.as_str())
        .count();
    Some(ResolvedInputDevice {
        uid,
        name,
        duplicate_name_count,
    })
}

fn property_address(
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        selector,
        scope,
        element: ELEMENT_MASTER,
    }
}

fn property_data_size(
    object_id: AudioObjectID,
    selector: AudioObjectPropertySelector,
    scope: AudioObjectPropertyScope,
) -> Option<u32> {
    let address = property_address(selector, scope);
    let mut size = 0u32;
    let status =
        unsafe { AudioObjectGetPropertyDataSize(object_id, &address, 0, ptr::null(), &mut size) };
    (status == AUDIO_NO_ERROR).then_some(size)
}

fn device_ids() -> Vec<AudioDeviceID> {
    let Some(mut size) = property_data_size(SYSTEM_OBJECT, PROP_DEVICES, SCOPE_GLOBAL) else {
        return Vec::new();
    };
    let count = size as usize / std::mem::size_of::<AudioDeviceID>();
    if count == 0 {
        return Vec::new();
    }

    let mut devices = Vec::<AudioDeviceID>::with_capacity(count);
    let address = property_address(PROP_DEVICES, SCOPE_GLOBAL);
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &address,
            0,
            ptr::null(),
            &mut size,
            devices.as_mut_ptr().cast::<c_void>(),
        )
    };
    if status != AUDIO_NO_ERROR {
        return Vec::new();
    }

    let returned = size as usize / std::mem::size_of::<AudioDeviceID>();
    unsafe {
        devices.set_len(returned.min(count));
    }
    devices
}

fn default_device(selector: AudioObjectPropertySelector) -> Option<AudioDeviceID> {
    let address = property_address(selector, SCOPE_GLOBAL);
    let mut size = std::mem::size_of::<AudioDeviceID>() as u32;
    let mut id = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &address,
            0,
            ptr::null(),
            &mut size,
            (&mut id as *mut AudioDeviceID).cast::<c_void>(),
        )
    };
    (status == AUDIO_NO_ERROR && id != 0).then_some(id)
}

fn has_streams(id: AudioDeviceID, scope: AudioObjectPropertyScope) -> bool {
    property_data_size(id, PROP_STREAMS, scope).is_some_and(|size| size > 0)
}

fn device_name(id: AudioDeviceID) -> Option<String> {
    device_cfstring_property(id, PROP_DEVICE_NAME_CFSTRING)
}

fn device_uid(id: AudioDeviceID) -> Option<String> {
    device_cfstring_property(id, PROP_DEVICE_UID)
}

fn device_cfstring_property(
    id: AudioDeviceID,
    selector: AudioObjectPropertySelector,
) -> Option<String> {
    let address = property_address(selector, SCOPE_GLOBAL);
    let mut size = std::mem::size_of::<*const c_void>() as u32;
    let mut string_ref: *const c_void = ptr::null();
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            &address,
            0,
            ptr::null(),
            &mut size,
            (&mut string_ref as *mut *const c_void).cast::<c_void>(),
        )
    };
    if status != AUDIO_NO_ERROR || string_ref.is_null() {
        return None;
    }

    let mut buffer = [0 as c_char; 1024];
    let ok = unsafe {
        CFStringGetCString(
            string_ref,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            CF_STRING_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        return None;
    }

    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_str()
        .ok()
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::traits::DeviceTrait;

    #[test]
    fn coreaudio_uid_device_id_round_trips() {
        let id = device_id_from_uid("AppleUSBAudioEngine:Example");
        assert_eq!(id, "coreaudio:uid:AppleUSBAudioEngine:Example");
        assert!(is_coreaudio_uid_id(&id));
        assert_eq!(uid_from_device_id(&id), Some("AppleUSBAudioEngine:Example"));
        assert_eq!(uid_from_device_id("coreaudio:Studio Display"), None);
    }

    /// Hardware-dependent: `enumerate()` enters CoreAudio. Keep this out of the
    /// default unit-test path so a wedged audio daemon cannot hang `cargo test`.
    #[test]
    #[ignore]
    fn enumerate_is_total() {
        let devices = enumerate();
        // The fields must be Vecs; both may be empty on headless CI.
        let _inputs: &Vec<DeviceInfo> = &devices.inputs;
        let _outputs: &Vec<DeviceInfo> = &devices.outputs;
    }

    /// Hardware-dependent: assert the lists are non-empty.
    /// Skip on CI / headless runners.
    #[test]
    #[ignore]
    fn enumerate_has_devices_on_real_hardware() {
        let devices = enumerate();
        assert!(
            !devices.inputs.is_empty() || !devices.outputs.is_empty(),
            "Expected at least one audio device on real hardware"
        );
    }

    /// Hardware-dependent: default device appears first when present.
    #[test]
    #[ignore]
    fn default_device_is_first() {
        use cpal::traits::HostTrait;
        let host = cpal::default_host();

        if let Some(default_in) = host.default_input_device() {
            if let Ok(default_name) = default_in.name() {
                let devices = enumerate();
                let first = devices.inputs.first().expect("inputs should be non-empty");
                assert_eq!(first.name, default_name);
            }
        }

        if let Some(default_out) = host.default_output_device() {
            if let Ok(default_name) = default_out.name() {
                let devices = enumerate();
                let first = devices
                    .outputs
                    .first()
                    .expect("outputs should be non-empty");
                assert_eq!(first.name, default_name);
            }
        }
    }

    /// Hardware-dependent: no duplicate ids in either list.
    #[test]
    #[ignore]
    fn no_duplicate_ids() {
        let devices = enumerate();
        let mut ids: Vec<&str> = devices.inputs.iter().map(|d| d.id.as_str()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate input device ids");

        let mut ids: Vec<&str> = devices.outputs.iter().map(|d| d.id.as_str()).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate output device ids");
    }
}
