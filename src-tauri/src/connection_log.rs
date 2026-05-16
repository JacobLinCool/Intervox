//! Connection lifecycle log: a bounded in-memory ring (newest-last) plus an
//! appended text file (size-capped). Used by the Advanced → View log modal.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;

pub const RING_CAP: usize = 200;
const FILE_CAP_BYTES: u64 = 1_000_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ConnLogEntry {
    pub ts: String,
    pub kind: String, // connecting|connected|reconnecting|failed|closed|error|latency
    pub detail: String,
}

#[derive(Default)]
pub struct ConnectionLog {
    ring: Mutex<VecDeque<ConnLogEntry>>,
}

fn log_path() -> PathBuf {
    let base = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("app.intervox.desktop").join("connection.log")
}

impl ConnectionLog {
    pub fn push(&self, kind: &str, detail: impl Into<String>) {
        let e = ConnLogEntry {
            ts: crate::commands::rfc3339_now(),
            kind: kind.to_string(),
            detail: detail.into(),
        };
        {
            let mut r = self.ring.lock();
            if r.len() == RING_CAP {
                r.pop_front();
            }
            r.push_back(e.clone());
        }
        let p = log_path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(meta) = std::fs::metadata(&p) {
            if meta.len() > FILE_CAP_BYTES {
                if let Ok(content) = std::fs::read(&p) {
                    let keep_from =
                        content.len().saturating_sub(FILE_CAP_BYTES as usize / 2);
                    // Trim to the start of the next line so the file stays line-aligned.
                    let slice = &content[keep_from..];
                    let start = slice
                        .iter()
                        .position(|&b| b == b'\n')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let _ = std::fs::write(&p, &slice[start..]);
                }
            }
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = writeln!(f, "{} [{}] {}", e.ts, e.kind, e.detail);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    #[allow(dead_code)]
    pub fn snapshot(&self) -> Vec<ConnLogEntry> {
        self.ring.lock().iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_caps_at_capacity_and_keeps_newest() {
        let log = ConnectionLog::default();
        for i in 0..(RING_CAP + 5) {
            log.push("connecting", i.to_string());
        }
        let s = log.snapshot();
        assert_eq!(s.len(), RING_CAP);
        assert_eq!(s.last().unwrap().detail, (RING_CAP + 4).to_string());
        assert_eq!(s.first().unwrap().detail, "5");
    }
}
