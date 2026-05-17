use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;

const APP_ID: &str = "app.intervox.desktop";
const LOCK_FILE: &str = "intervox.lock";

pub struct SingleInstanceGuard {
    file: File,
}

#[derive(Debug)]
pub enum SingleInstanceError {
    AlreadyRunning { pid: Option<u32>, source: io::Error },
    Io(io::Error),
}

impl std::fmt::Display for SingleInstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning { pid, source } => {
                write!(f, "another Intervox instance is already running")?;
                if let Some(pid) = pid {
                    write!(f, " (pid {pid})")?;
                }
                write!(f, ": {source}")
            }
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SingleInstanceError {}

impl From<io::Error> for SingleInstanceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn acquire() -> Result<SingleInstanceGuard, SingleInstanceError> {
    let dir = lock_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(LOCK_FILE);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if let Err(source) = try_lock_exclusive(&file) {
        return Err(SingleInstanceError::AlreadyRunning {
            pid: read_pid(&mut file).ok().flatten(),
            source,
        });
    }
    file.set_len(0)?;
    writeln!(file, "{}", std::process::id())?;
    Ok(SingleInstanceGuard { file })
}

pub fn activate_existing(pid: Option<u32>) {
    let fallback = pid
        .map(|pid| {
            format!(
                r#"tell application "System Events"
  set frontmost of first application process whose unix id is {pid} to true
end tell"#
            )
        })
        .unwrap_or_default();
    let script = format!(
        r#"try
  tell application id "{APP_ID}"
    activate
    reopen
  end tell
on error
{fallback}
end try"#
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn();
}

fn lock_dir() -> io::Result<PathBuf> {
    dirs::data_dir()
        .map(|dir| dir.join(APP_ID))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "user data directory not found"))
}

fn read_pid(file: &mut File) -> io::Result<Option<u32>> {
    file.seek(SeekFrom::Start(0))?;
    let mut raw = String::new();
    file.read_to_string(&mut raw)?;
    Ok(raw.trim().parse::<u32>().ok())
}

fn try_lock_exclusive(file: &File) -> io::Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn unlock(file: &File) {
    let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unlock(&self.file);
    }
}
