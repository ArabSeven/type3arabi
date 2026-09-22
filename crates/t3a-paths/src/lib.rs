//! Paths and the per-user directory (docs/01 §5).

use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "Type3arabi";
pub const DATA_FILE_NAME: &str = "type3arabi.dat";
pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const STATE_FILE_NAME: &str = "state.toml";
pub const USER_SUBDIR: &str = "user";
pub const JOURNAL_FILE_NAME: &str = "journal.t3j";
pub const SNAPSHOT_FILE_NAME: &str = "snapshot.t3u";
pub const LOG_SUBDIR: &str = "logs";
pub const ERROR_LOG_NAME: &str = "errors.log";
pub const ERROR_LOG_MAX_BYTES: u64 = 256 * 1024;
/// SDDL SIDs granted read access to the user dir.
pub const APPCONTAINER_SIDS: [&str; 2] = ["S-1-15-2-1", "S-1-15-2-2"];
/// Named mutex for journal compaction (docs/03 §9.4).
pub const COMPACTION_MUTEX: &str = "Local\\Type3arabi.UserStore";

#[cfg(windows)]
pub fn install_dir() -> PathBuf {
    use std::env;
    // On 64-bit Windows, both x64 and x86 processes should access %ProgramFiles%\Type3arabi (not Program Files (x86))
    if let Ok(pf64) = env::var("ProgramW6432") {
        PathBuf::from(pf64).join(APP_DIR_NAME)
    } else if let Ok(pf) = env::var("ProgramFiles") {
        PathBuf::from(pf).join(APP_DIR_NAME)
    } else {
        PathBuf::from(r"C:\Program Files").join(APP_DIR_NAME)
    }
}

#[cfg(not(windows))]
pub fn install_dir() -> PathBuf {
    PathBuf::from("/usr/share").join(APP_DIR_NAME)
}

pub fn data_file_path() -> PathBuf {
    install_dir().join(DATA_FILE_NAME)
}

#[cfg(windows)]
pub fn user_dir() -> PathBuf {
    use std::env;
    if let Ok(local) = env::var("LOCALAPPDATA") {
        PathBuf::from(local).join(APP_DIR_NAME)
    } else if let Ok(userprofile) = env::var("USERPROFILE") {
        PathBuf::from(userprofile)
            .join("AppData")
            .join("Local")
            .join(APP_DIR_NAME)
    } else {
        PathBuf::from(r"C:\").join(APP_DIR_NAME)
    }
}

#[cfg(not(windows))]
pub fn user_dir() -> PathBuf {
    PathBuf::from("/tmp").join(APP_DIR_NAME)
}

pub fn user_store_dir() -> PathBuf {
    user_dir().join(USER_SUBDIR)
}

pub fn logs_dir() -> PathBuf {
    user_dir().join(LOG_SUBDIR)
}

pub fn error_log_path() -> PathBuf {
    logs_dir().join(ERROR_LOG_NAME)
}

pub fn config_path() -> PathBuf {
    user_dir().join(CONFIG_FILE_NAME)
}

#[cfg(windows)]
pub fn is_app_container() -> bool {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Security::{GetTokenInformation, TokenIsAppContainer, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: Querying current process token information
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_ok() {
            let mut is_ac: u32 = 0;
            let mut ret_len: u32 = 0;
            let ok = GetTokenInformation(
                token,
                TokenIsAppContainer,
                Some(&mut is_ac as *mut _ as *mut _),
                std::mem::size_of::<u32>() as u32,
                &mut ret_len,
            );
            let _ = windows::Win32::Foundation::CloseHandle(token);
            if ok.is_ok() {
                return is_ac != 0;
            }
        }
    }
    false
}

#[cfg(not(windows))]
pub fn is_app_container() -> bool {
    false
}

/// Creates `%LOCALAPPDATA%\Type3arabi`, `user\`, and `logs\`.
/// Grants read access to AppContainer packages via icacls if on Windows and not in AppContainer.
pub fn ensure_user_dir() -> std::io::Result<()> {
    let u_dir = user_dir();
    std::fs::create_dir_all(&u_dir)?;
    std::fs::create_dir_all(user_store_dir())?;
    std::fs::create_dir_all(logs_dir())?;

    #[cfg(windows)]
    {
        if !is_app_container() {
            // Apply read permissions for ALL APPLICATION PACKAGES (S-1-15-2-1)
            // and ALL RESTRICTED APPLICATION PACKAGES (S-1-15-2-2)
            // Using icacls via Command without displaying console window
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            for sid in APPCONTAINER_SIDS {
                let _ = std::process::Command::new("icacls.exe")
                    .arg(&u_dir)
                    .arg("/grant")
                    .arg(format!("*{sid}:(OI)(CI)(RX)"))
                    .arg("/T")
                    .arg("/Q")
                    .creation_flags(CREATE_NO_WINDOW)
                    .status();
            }
        }
    }

    Ok(())
}

/// Append one line to `logs\errors.log` (max 256 KB, rotated once).
/// NEVER includes user text (AGENTS.md R8).
pub fn log_error(msg: &str) {
    let log_file = error_log_path();
    if let Some(parent) = log_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Check size for rotation
    if let Ok(metadata) = std::fs::metadata(&log_file) {
        if metadata.len() > ERROR_LOG_MAX_BYTES {
            let backup = log_file.with_extension("log.old");
            let _ = std::fs::rename(&log_file, backup);
        }
    }

    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{now}] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_sane() {
        let idir = install_dir();
        assert!(idir.to_string_lossy().contains("Type3arabi"));
        let udir = user_dir();
        assert!(udir.to_string_lossy().contains("Type3arabi"));
        let data_path = data_file_path();
        assert!(data_path.ends_with(DATA_FILE_NAME));
    }

    #[test]
    fn test_error_logging() {
        let msg = "test error event without user text";
        log_error(msg);
        let log = error_log_path();
        assert!(log.exists());
        let content = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(content.contains(msg));
    }
}
