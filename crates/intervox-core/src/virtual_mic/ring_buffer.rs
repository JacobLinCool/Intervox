//! Shared single-producer/single-consumer audio ring buffer (spec §9.4).
//!
//! Layout is `#[repr(C)]` so the same memory can be mmap'd by the Core Audio
//! HAL driver. The driver is the consumer on its realtime render thread: the
//! read path never allocates, never blocks, and yields silence on underrun
//! (non-negotiable rules §19.4, §19.5).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// 48 kHz mono * 8 seconds (spec §9.4).
pub const RING_BUFFER_CAPACITY: usize = 384_000;
/// Live virtual-mic output must stay current.  The 8 s capacity is a safety
/// window, not a FIFO latency budget; once the consumer falls behind, keep only
/// the newest 100 ms and drop older unread audio.
pub const LIVE_MAX_UNREAD_FRAMES: usize = 4_800;
/// ASCII "IVOX".
pub const RING_MAGIC: u32 = 0x49_56_4F_58;
pub const RING_VERSION: u32 = 2;

#[repr(C)]
pub struct RingSlot {
    sequence: AtomicU64,
    bits: AtomicU32,
    _pad: u32,
}

#[repr(C)]
pub struct SharedAudioRingBuffer {
    pub magic: u32,
    pub version: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub capacity_frames: u64,
    pub write_index: AtomicU64,
    pub read_index: AtomicU64,
    pub generation: AtomicU64,
    pub mode: AtomicU32,
    _pad: u32,
    slots: [RingSlot; RING_BUFFER_CAPACITY],
}

// Safe: every slot is atomic and indices are advanced monotonically. The
// producer may drop old unread frames to keep the virtual mic current.
unsafe impl Sync for SharedAudioRingBuffer {}

impl SharedAudioRingBuffer {
    /// Allocate zeroed on the heap (the frame array is ~1.5 MiB — never build
    /// it on the stack) and initialise the header.
    pub fn new_boxed(sample_rate: u32, channels: u32) -> Box<Self> {
        use std::alloc::{alloc_zeroed, Layout};
        let layout = Layout::new::<Self>();
        // SAFETY: all-zero is a valid bit pattern for every field. We then
        // overwrite the header. Box takes ownership.
        let b = unsafe {
            let ptr = alloc_zeroed(layout) as *mut Self;
            assert!(!ptr.is_null(), "ring buffer allocation failed");
            Box::from_raw(ptr)
        };
        b.magic_init(sample_rate, channels);
        b
    }

    fn magic_init(&self, sample_rate: u32, channels: u32) {
        // Header fields are written once at init before any sharing.
        let this = self as *const Self as *mut Self;
        unsafe {
            (*this).magic = RING_MAGIC;
            (*this).version = RING_VERSION;
            (*this).sample_rate = sample_rate;
            (*this).channels = channels;
            (*this).capacity_frames = RING_BUFFER_CAPACITY as u64;
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == RING_MAGIC
            && self.version == RING_VERSION
            && self.sample_rate == 48_000
            && self.channels == 1
            && self.capacity_frames == RING_BUFFER_CAPACITY as u64
    }

    fn cap(&self) -> u64 {
        RING_BUFFER_CAPACITY as u64
    }

    pub fn available_to_read(&self) -> u64 {
        let w = self.write_index.load(Ordering::Acquire);
        let r = self.read_index.load(Ordering::Acquire);
        w.wrapping_sub(r)
    }

    pub fn recent_max_abs(&self, frames: usize) -> f32 {
        let w = self.write_index.load(Ordering::Acquire);
        let n = frames.min(RING_BUFFER_CAPACITY);
        let start = w.saturating_sub(n as u64);
        let mut max_abs = 0.0f32;
        for i in 0..n {
            let sample = self.read_slot(start + i as u64).unwrap_or(0.0);
            max_abs = max_abs.max(sample.abs());
        }
        max_abs
    }

    pub fn set_mode(&self, mode: u32) {
        self.mode.store(mode, Ordering::Release);
    }

    fn advance_read_index_to(&self, target: u64) {
        let mut current = self.read_index.load(Ordering::Acquire);
        while target > current {
            match self.read_index.compare_exchange_weak(
                current,
                target,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(next) => current = next,
            }
        }
    }

    fn trim_unread_to(&self, max_frames: u64) {
        let w = self.write_index.load(Ordering::Acquire);
        let r = self.read_index.load(Ordering::Acquire);
        let unread = w.wrapping_sub(r);
        if unread > max_frames {
            self.advance_read_index_to(w - max_frames);
        }
    }

    fn write_slot(&self, index: u64, sample: f32) {
        let slot = &self.slots[(index % self.cap()) as usize];
        let seq = index << 1;
        slot.sequence.store(seq | 1, Ordering::Release);
        slot.bits.store(sample.to_bits(), Ordering::Relaxed);
        slot.sequence.store(seq, Ordering::Release);
    }

    fn read_slot(&self, index: u64) -> Option<f32> {
        let slot = &self.slots[(index % self.cap()) as usize];
        let expected = index << 1;
        let before = slot.sequence.load(Ordering::Acquire);
        if before != expected {
            return None;
        }
        let bits = slot.bits.load(Ordering::Relaxed);
        let after = slot.sequence.load(Ordering::Acquire);
        (after == before).then(|| f32::from_bits(bits))
    }

    /// Producer side. Writes the newest samples and drops the oldest unread
    /// frames when necessary. A virtual microphone must stay current; preserving
    /// minutes-old unread audio is both incorrect and privacy-hostile.
    pub fn write_frames(&self, data: &[f32]) -> usize {
        if data.is_empty() {
            return 0;
        }
        let data = if data.len() > RING_BUFFER_CAPACITY {
            &data[data.len() - RING_BUFFER_CAPACITY..]
        } else {
            data
        };

        let w = self.write_index.load(Ordering::Relaxed);
        let r = self.read_index.load(Ordering::Acquire);
        let unread = w.saturating_sub(r).min(self.cap());
        let n = data.len();
        let required = unread + n as u64;
        if required > self.cap() {
            self.advance_read_index_to(r + (required - self.cap()));
        }

        for (i, &s) in data.iter().enumerate() {
            self.write_slot(w + i as u64, s);
        }
        self.write_index
            .store(w.wrapping_add(n as u64), Ordering::Release);
        n
    }

    /// Producer side for continuous live audio.  Writes the newest samples and
    /// then bounds unread backlog to `LIVE_MAX_UNREAD_FRAMES`, so a paused or
    /// slow HAL consumer cannot later play seconds of stale microphone audio.
    pub fn write_live_frames(&self, data: &[f32]) -> usize {
        let n = self.write_frames(data);
        self.trim_unread_to(LIVE_MAX_UNREAD_FRAMES as u64);
        n
    }

    /// Drop all unread frames without touching sample memory.
    pub fn clear(&self) {
        let w = self.write_index.load(Ordering::Acquire);
        self.advance_read_index_to(w);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Consumer side (driver render thread). Fills `out` entirely; missing
    /// samples become silence. Returns true if an underrun occurred. No
    /// allocation, no blocking.
    pub fn read_into(&self, out: &mut [f32]) -> bool {
        let r = self.read_index.load(Ordering::Relaxed);
        let w = self.write_index.load(Ordering::Acquire);
        let avail = w.wrapping_sub(r);
        let n = (out.len() as u64).min(avail) as usize;
        for (i, slot_out) in out.iter_mut().enumerate() {
            if (i as u64) < n as u64 {
                *slot_out = self.read_slot(r + i as u64).unwrap_or(0.0);
            } else {
                *slot_out = 0.0;
            }
        }
        self.advance_read_index_to(r.wrapping_add(n as u64));
        n < out.len()
    }
}

/// Byte size of the shared region — must match the C driver's
/// `sizeof(intervox_ring_t)` exactly.
pub const SHARED_REGION_BYTES: usize = std::mem::size_of::<SharedAudioRingBuffer>();

/// Default POSIX shared-memory object name (macOS limits to 31 chars,
/// must start with '/'). The HAL driver opens the same name.
pub const DEFAULT_SHM_NAME: &str = "/intervox.ring";

/// RAII mmap of the ring buffer in POSIX shared memory. The app is the
/// producer/creator; the Core Audio HAL driver (a separate `coreaudiod`
/// process) opens the same object as consumer.
#[cfg(unix)]
pub struct SharedRingMap {
    ptr: *mut SharedAudioRingBuffer,
    len: usize,
    fd: libc::c_int,
    name: std::ffi::CString,
    owner: bool,
}

#[cfg(unix)]
// Safe to move across threads: it is just a pointer to shared memory whose
// concurrent access is governed by the atomic indices.
unsafe impl Send for SharedRingMap {}

#[cfg(unix)]
impl SharedRingMap {
    fn norm_name(name: &str) -> std::ffi::CString {
        let n = if name.starts_with('/') {
            name.to_string()
        } else {
            format!("/{name}")
        };
        std::ffi::CString::new(n).expect("shm name has no NUL")
    }

    fn map(fd: libc::c_int) -> std::io::Result<(*mut SharedAudioRingBuffer, usize)> {
        let len = SHARED_REGION_BYTES;
        // SAFETY: fd is a valid shm fd sized to `len`; we map the whole region.
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error());
        }
        Ok((p as *mut SharedAudioRingBuffer, len))
    }

    /// Producer side: create (or recreate) the shared object and init header.
    pub fn create(name: &str, sample_rate: u32, channels: u32) -> std::io::Result<Self> {
        let cname = Self::norm_name(name);
        unsafe { libc::shm_unlink(cname.as_ptr()) }; // clear any stale object
                                                     // SAFETY: standard shm_open with explicit mode.
                                                     // The HAL driver runs as a different uid (`_coreaudiod`), so the
                                                     // object must be group/other accessible. macOS honors the shm_open
                                                     // mode argument (subject to umask); `fchmod` on a shm fd returns
                                                     // EINVAL here, so the mode arg is the right lever.
        let old_umask = unsafe { libc::umask(0) };
        let fd = unsafe {
            libc::shm_open(
                cname.as_ptr(),
                libc::O_CREAT | libc::O_RDWR | libc::O_EXCL,
                0o666,
            )
        };
        unsafe { libc::umask(old_umask) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { libc::ftruncate(fd, SHARED_REGION_BYTES as libc::off_t) } != 0 {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }
        let (ptr, len) = Self::map(fd).inspect_err(|_| unsafe {
            libc::close(fd);
        })?;
        // Zero-filled by the OS; stamp the header so the driver can validate.
        unsafe { (*ptr).magic_init(sample_rate, channels) };
        Ok(Self {
            ptr,
            len,
            fd,
            name: cname,
            owner: true,
        })
    }

    /// Consumer side (the driver mirrors this in C): open an existing object.
    pub fn open(name: &str) -> std::io::Result<Self> {
        let cname = Self::norm_name(name);
        let fd = unsafe { libc::shm_open(cname.as_ptr(), libc::O_RDWR, 0o600) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let (ptr, len) = Self::map(fd).inspect_err(|_| unsafe {
            libc::close(fd);
        })?;
        let map = Self {
            ptr,
            len,
            fd,
            name: cname,
            owner: false,
        };
        if !map.get().is_valid() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ring buffer header invalid (magic/version mismatch)",
            ));
        }
        Ok(map)
    }

    pub fn get(&self) -> &SharedAudioRingBuffer {
        // SAFETY: ptr is a valid mapping for the lifetime of self.
        unsafe { &*self.ptr }
    }
}

#[cfg(unix)]
impl Drop for SharedRingMap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.len);
            libc::close(self.fd);
            if self.owner {
                libc::shm_unlink(self.name.as_ptr());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_valid_and_spec_sized() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        assert!(rb.is_valid());
        assert_eq!(rb.sample_rate, 48000);
        assert_eq!(rb.channels, 1);
        assert_eq!(rb.capacity_frames, 384_000);
        assert_eq!(RING_BUFFER_CAPACITY, 48_000 * 8);
    }

    #[test]
    fn spsc_round_trip_in_order() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        let data: Vec<f32> = (0..1000).map(|i| i as f32).collect();
        assert_eq!(rb.write_frames(&data), 1000);
        let mut out = vec![-1.0; 1000];
        let underrun = rb.read_into(&mut out);
        assert!(!underrun);
        assert_eq!(out, data);
    }

    #[test]
    fn empty_read_is_silence_and_underrun() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        let mut out = vec![9.0; 256];
        let underrun = rb.read_into(&mut out);
        assert!(underrun);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn partial_read_pads_with_silence() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        rb.write_frames(&[1.0, 2.0, 3.0]);
        let mut out = vec![-1.0; 5];
        let underrun = rb.read_into(&mut out);
        assert!(underrun);
        assert_eq!(out, vec![1.0, 2.0, 3.0, 0.0, 0.0]);
    }

    #[test]
    fn overflow_keeps_newest_window() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        let data: Vec<f32> = (0..(RING_BUFFER_CAPACITY + 2)).map(|i| i as f32).collect();
        assert_eq!(rb.write_frames(&data), RING_BUFFER_CAPACITY);

        let mut out = vec![0.0; RING_BUFFER_CAPACITY];
        assert!(!rb.read_into(&mut out));
        assert_eq!(out[0], 2.0);
        assert_eq!(
            out[RING_BUFFER_CAPACITY - 1],
            (RING_BUFFER_CAPACITY + 1) as f32
        );
    }

    #[test]
    fn live_writes_keep_only_recent_audio() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        let data: Vec<f32> = (0..(LIVE_MAX_UNREAD_FRAMES + 1000))
            .map(|i| i as f32)
            .collect();

        assert_eq!(rb.write_live_frames(&data), data.len());
        assert_eq!(rb.available_to_read(), LIVE_MAX_UNREAD_FRAMES as u64);

        let mut out = vec![0.0; LIVE_MAX_UNREAD_FRAMES];
        assert!(!rb.read_into(&mut out));
        assert_eq!(out[0], 1000.0);
        assert_eq!(out[LIVE_MAX_UNREAD_FRAMES - 1], (data.len() - 1) as f32);
    }

    #[test]
    fn repeated_live_writes_do_not_accumulate_seconds_of_backlog() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        let chunk = vec![1.0f32; 480];

        for _ in 0..1000 {
            rb.write_live_frames(&chunk);
        }

        assert_eq!(rb.available_to_read(), LIVE_MAX_UNREAD_FRAMES as u64);
    }

    #[test]
    fn clear_discards_unread_audio() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        rb.write_frames(&[1.0, 2.0, 3.0]);
        rb.clear();
        rb.write_frames(&[0.0, 0.0]);

        let mut out = vec![9.0; 3];
        assert!(rb.read_into(&mut out));
        assert_eq!(out, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn wrap_around_is_correct() {
        let rb = SharedAudioRingBuffer::new_boxed(48000, 1);
        // Drive write/read indices near capacity then cross the boundary.
        let chunk = vec![7.0f32; 100_000];
        for _ in 0..4 {
            rb.write_frames(&chunk);
            let mut o = vec![0.0; 100_000];
            assert!(!rb.read_into(&mut o));
            assert!(o.iter().all(|&s| s == 7.0));
        }
        // Crossed 384_000 boundary at least once.
        assert!(rb.write_index.load(Ordering::Relaxed) > RING_BUFFER_CAPACITY as u64);
    }

    #[test]
    fn shared_region_size_matches_c_layout() {
        // header (56 bytes) + 384_000 sequence-tagged slots (16 bytes each)
        assert_eq!(std::mem::size_of::<RingSlot>(), 16);
        assert_eq!(SHARED_REGION_BYTES, 56 + RING_BUFFER_CAPACITY * 16);
        assert_eq!(SHARED_REGION_BYTES, 6_144_056);
    }

    #[cfg(unix)]
    #[test]
    fn shm_create_then_open_round_trips() {
        let name = format!("/ivx-test-{}", std::process::id());
        let producer = SharedRingMap::create(&name, 48000, 1).expect("create shm");
        let consumer = SharedRingMap::open(&name).expect("open shm");
        assert!(consumer.get().is_valid());
        assert_eq!(consumer.get().sample_rate, 48000);

        let data: Vec<f32> = (0..256).map(|i| i as f32 / 256.0).collect();
        assert_eq!(producer.get().write_frames(&data), 256);
        let mut out = vec![0.0; 256];
        assert!(!consumer.get().read_into(&mut out));
        assert_eq!(out, data);
    }
}
