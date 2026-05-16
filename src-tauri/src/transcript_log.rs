//! Per-session transcript persistence as JSON Lines.
//! Dir: ~/Library/Application Support/app.intervox.desktop/transcripts/ (0700)
//! File: <fs-safe RFC3339 session start>.jsonl (0600), one record per line.

use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TranscriptRecord {
    pub ts: String,   // RFC3339 UTC
    pub kind: String, // "source" | "target"
    pub lang: String,
    pub text: String,
}

pub fn transcripts_dir() -> PathBuf {
    let base = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"));
    base.join("app.intervox.desktop").join("transcripts")
}

fn fs_safe(ts: &str) -> String {
    ts.replace(':', "-")
}

/// Open/append a per-session file. None until a session starts.
#[derive(Default)]
pub struct SessionLog {
    path: Mutex<Option<PathBuf>>,
}

impl SessionLog {
    pub fn start(&self, session_start_rfc3339: &str) {
        let dir = transcripts_dir();
        let _ = std::fs::create_dir_all(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        let p = dir.join(format!("{}.jsonl", fs_safe(session_start_rfc3339)));
        *self.path.lock().unwrap() = Some(p);
    }

    pub fn end(&self) {
        *self.path.lock().unwrap() = None;
    }

    /// Append one record. Best-effort: a write failure is swallowed.
    pub fn append(&self, rec: &TranscriptRecord) {
        let guard = self.path.lock().unwrap();
        let Some(p) = guard.as_ref() else { return };
        if let Ok(line) = serde_json::to_string(rec) {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
            {
                let _ = writeln!(f, "{line}");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        p,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }
            }
        }
    }
}

/// Delete every file under the transcripts dir. Returns count removed.
pub fn clear_all() -> usize {
    let dir = transcripts_dir();
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            if std::fs::remove_file(e.path()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_serializes_compact_jsonl() {
        let r = TranscriptRecord {
            ts: "2026-05-17T00:00:00Z".into(),
            kind: "target".into(),
            lang: "en".into(),
            text: "hello".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        assert_eq!(
            s,
            r#"{"ts":"2026-05-17T00:00:00Z","kind":"target","lang":"en","text":"hello"}"#
        );
    }

    #[test]
    fn fs_safe_replaces_colons() {
        assert_eq!(fs_safe("2026-05-17T12:34:56Z"), "2026-05-17T12-34-56Z");
    }
}
