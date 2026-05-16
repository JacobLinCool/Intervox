//! App-side POSIX shared-memory ring producer.
//!
//! `RingProducer` owns a `SharedRingMap` behind a `parking_lot::Mutex` so that
//! `RingProducer: Send + Sync` and can be stored in Tauri managed state even
//! though `SharedRingMap` is `Send` but not `Sync`.

use intervox_core::virtual_mic::ring_buffer::{SharedRingMap, DEFAULT_SHM_NAME};

use super::translate_chain;

/// Wraps the POSIX shm ring map behind a mutex so the producer is `Sync`.
///
/// `SharedRingMap` is `Send` but not `Sync`.  Storing it inside a
/// `parking_lot::Mutex<SharedRingMap>` yields `Send + Sync` for the `Mutex`
/// (because `SharedRingMap: Send`), which satisfies `tauri::Manager::manage`.
pub struct RingProducer {
    map: parking_lot::Mutex<SharedRingMap>,
}

// parking_lot::Mutex<SharedRingMap> is Send + Sync when SharedRingMap: Send,
// so RingProducer inherits Send + Sync automatically — no unsafe needed.

impl RingProducer {
    /// Open-or-create the POSIX shm ring at `DEFAULT_SHM_NAME`.
    ///
    /// `SharedRingMap::create` already `shm_unlink`s any stale object first,
    /// so there is no separate stale-recreate logic required.
    pub fn open_or_create() -> std::io::Result<Self> {
        let map = SharedRingMap::create(DEFAULT_SHM_NAME, 48_000, 1)?;
        Ok(Self {
            map: parking_lot::Mutex::new(map),
        })
    }

    /// Write PCM frames to the ring.  Returns the number of frames accepted
    /// (may be less than `frames.len()` if the buffer is nearly full).
    // Used by Task 3.3 capture pipeline.
    #[allow(dead_code)]
    pub fn write(&self, frames: &[f32]) -> usize {
        self.map.lock().get().write_frames(frames)
    }

    /// Write continuous live audio and cap unread backlog to the live budget.
    #[allow(dead_code)]
    pub fn write_live(&self, frames: &[f32]) -> usize {
        self.map.lock().get().write_live_frames(frames)
    }

    /// Drop unread audio without writing replacement frames.
    pub fn clear_unread(&self) {
        self.map.lock().get().clear();
    }

    /// Signal the driver about the current operating mode.
    /// Use `mode_to_ring_u32` to convert a `VirtualMicMode`.
    pub fn set_mode(&self, m: u32) {
        self.map.lock().get().set_mode(m);
    }

    /// Return the number of unread frames in the ring as milliseconds at 48 kHz.
    ///
    /// This is the "virtual mic output lag": frames that have been written by the
    /// pull task but not yet consumed by the HAL driver.  The lock hold-time is
    /// sub-microsecond (one `available_to_read()` atomic read).
    pub fn backlog_ms(&self) -> u32 {
        let guard = self.map.lock();
        let frames = guard.get().available_to_read();
        translate_chain::frames_to_ms_48k(frames)
    }

    /// Drop any unread audio and publish fresh silence so the driver consumer
    /// never receives stale captured frames after a mode change or shutdown.
    ///
    /// Called on shutdown and on a mode-change to `Silence`.
    pub fn flush_silence(&self) {
        const CHUNK: usize = 4_800; // 100 ms at 48 kHz mono
        let silence = vec![0.0f32; CHUNK];
        let guard = self.map.lock();
        let ring = guard.get();
        ring.clear();
        for _ in 0..3 {
            ring.write_frames(&silence);
        }
    }
}

/// Pure mapping from `VirtualMicMode` to the u32 hint stored in the ring
/// header's `mode` atomic, which the HAL driver reads for its own heuristics.
///
/// | Mode                  | u32 |
/// |-----------------------|-----|
/// | Silence               |  0  |
/// | PassThrough           |  1  |
/// | Translate             |  2  |
/// | TranslateWithOriginal |  3  |
pub fn mode_to_ring_u32(mode: intervox_core::state::VirtualMicMode) -> u32 {
    use intervox_core::state::VirtualMicMode::*;
    match mode {
        Silence => 0,
        PassThrough => 1,
        Translate => 2,
        TranslateWithOriginal => 3,
    }
}

/// Inverse of `mode_to_ring_u32`.  Any value not in {1, 2, 3} maps to
/// `Silence` (defensive default).
///
/// | u32 | Mode                  |
/// |-----|-----------------------|
/// |   3 | TranslateWithOriginal |
/// |   2 | Translate             |
/// |   1 | PassThrough           |
/// |   _ | Silence               |
pub fn mode_from_u32(v: u32) -> intervox_core::state::VirtualMicMode {
    use intervox_core::state::VirtualMicMode::*;
    match v {
        3 => TranslateWithOriginal,
        2 => Translate,
        1 => PassThrough,
        _ => Silence,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use intervox_core::state::VirtualMicMode;

    /// Pure test — no shm, no side effects.
    #[test]
    fn mode_to_ring_u32_mapping_is_correct() {
        assert_eq!(mode_to_ring_u32(VirtualMicMode::Silence), 0);
        assert_eq!(mode_to_ring_u32(VirtualMicMode::PassThrough), 1);
        assert_eq!(mode_to_ring_u32(VirtualMicMode::Translate), 2);
        assert_eq!(mode_to_ring_u32(VirtualMicMode::TranslateWithOriginal), 3);
    }

    /// Round-trip: `mode_from_u32(mode_to_ring_u32(m)) == m` for all 4 modes.
    #[test]
    fn mode_round_trip() {
        use super::mode_from_u32;
        let modes = [
            VirtualMicMode::Silence,
            VirtualMicMode::PassThrough,
            VirtualMicMode::Translate,
            VirtualMicMode::TranslateWithOriginal,
        ];
        for m in modes {
            assert_eq!(
                mode_from_u32(mode_to_ring_u32(m)),
                m,
                "round-trip failed for {m:?}"
            );
        }
    }

    /// Defensive: unknown u32 values map to Silence.
    #[test]
    fn mode_from_u32_unknown_maps_to_silence() {
        use super::mode_from_u32;
        assert_eq!(mode_from_u32(99), VirtualMicMode::Silence);
        assert_eq!(mode_from_u32(u32::MAX), VirtualMicMode::Silence);
    }

    /// Creates a real `/intervox.ring` shm object — must run as a user with
    /// POSIX shm write access.  Excluded from CI (`#[ignore]`).
    #[test]
    #[ignore]
    fn shm_ring_producer_write_returns_count() {
        let producer = RingProducer::open_or_create().expect("create ring");
        let silence = vec![0.0f32; 480];
        let written = producer.write(&silence);
        assert_eq!(written, 480, "all frames should fit in a fresh ring");
    }
}
