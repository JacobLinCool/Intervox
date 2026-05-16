//! intervox-core — designer-independent core for the Intervox realtime
//! speech-translation virtual-mic app.
//!
//! Pure logic only: no audio I/O, no network, no platform calls. Everything
//! here is unit-testable and exercised by the `intervox-cli` verification
//! harness so it can be validated before the UI / driver / network layers
//! are wired in.

pub mod errors;
pub mod state;
pub mod config;
pub mod pipeline;

pub mod audio {
    pub mod pcm;
    pub mod resampler;
    pub mod mixer;
    pub mod level_meter;
    pub mod vad;
    pub mod jitter_buffer;
    pub mod delay_line;
}

pub mod realtime {
    pub mod events;
}

pub mod captions {
    pub mod transcript_state;
}

pub mod diagnostics {
    pub mod metrics;
}

pub mod virtual_mic {
    pub mod ring_buffer;
}

pub use config::Config;
pub use errors::{AppError, AppErrorCode, RecoveryAction};
pub use pipeline::{route, RouteDecision};
pub use state::{AppState, AppStatus, Health, VirtualMicMode};
