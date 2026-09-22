//! Paths and the per-user directory (docs/01 §5).
//!
//! Portable constants below; Windows functions (M1/M4):
//! - `install_dir()` → `%ProgramFiles%\Type3arabi` (FOLDERID_ProgramFiles; x86 builds must still resolve the
//!   64-bit Program Files on 64-bit Windows: use FOLDERID_ProgramFilesX64 when running under WOW64).
//! - `user_dir()` → `%LOCALAPPDATA%\Type3arabi` (FOLDERID_LocalAppData).
//! - `ensure_user_dir()` → create `user_dir()`, `user\`, `logs\`; add inheritable read/execute ACEs for
//!   `S-1-15-2-1` (ALL APPLICATION PACKAGES) and `S-1-15-2-2` (ALL RESTRICTED APPLICATION PACKAGES).
//!   Never called from an AppContainer process.
//! - `log_error(msg)` → append one line to `logs\errors.log` (≤ 256 KB, rotate once). NEVER user text (R8).

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
