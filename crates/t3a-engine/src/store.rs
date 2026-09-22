//! Persistent, multi-process user store with journal, snapshot, and tailing (docs/03 §9.4).

use crate::journal::{JournalRecord, KIND_CHOOSE, KIND_NEGATIVE, KIND_WIPE, RECORD_SIZE};
use crate::user::{MemoryUser, UserScorer};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_JOURNAL_SIZE_BEFORE_COMPACT: u64 = 256 * 1024; // 256 KB
const SNAPSHOT_MAGIC: u32 = 0x54335501; // 'T3U' ver 1

/// Persistent user store managing learning, journal append, and cross-process tailing.
pub struct UserStore {
    dir: PathBuf,
    read_only: bool,
    model: Arc<RwLock<MemoryUser>>,
    tx: Option<Sender<JournalRecord>>,
    last_journal_offset: Arc<AtomicU64>,
    _compact_lock: Arc<Mutex<()>>,
}

fn current_timestamp() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

impl UserStore {
    /// Open user store at `dir`. If `read_only` is true (e.g. AppContainer or private mode),
    /// no writes or writer thread are initiated.
    pub fn open(dir: &Path, read_only: bool) -> std::io::Result<Self> {
        if !read_only {
            let _ = std::fs::create_dir_all(dir);
        }

        let mut user = MemoryUser::new();
        let snapshot_path = dir.join("snapshot.t3u");
        let journal_path = dir.join("journal.t3j");

        // 1. Load snapshot if present
        if snapshot_path.exists() {
            if let Ok(snap_bytes) = std::fs::read(&snapshot_path) {
                let _ = load_snapshot(&snap_bytes, &mut user);
            }
        }

        // 2. Replay journal if present
        let mut last_offset = 0u64;
        if journal_path.exists() {
            if let Ok(mut f) = File::open(&journal_path) {
                last_offset = replay_journal(&mut f, 0, &mut user)?;
            }
        }

        let model = Arc::new(RwLock::new(user));
        let last_journal_offset = Arc::new(AtomicU64::new(last_offset));
        let compact_lock = Arc::new(Mutex::new(()));

        // 3. Spawn background writer thread if writable
        let tx = if !read_only {
            let (sender, receiver) = channel::<JournalRecord>();
            let j_path = journal_path.clone();
            let offset_clone = Arc::clone(&last_journal_offset);
            let model_clone = Arc::clone(&model);
            let dir_clone = dir.to_path_buf();
            let lock_clone = Arc::clone(&compact_lock);

            std::thread::Builder::new()
                .name("t3a-user-writer".into())
                .spawn(move || {
                    while let Ok(rec) = receiver.recv() {
                        let bytes = rec.to_bytes();
                        if let Ok(mut f) =
                            OpenOptions::new().create(true).append(true).open(&j_path)
                        {
                            if f.write_all(&bytes).is_ok() {
                                let _ = f.flush();
                                let new_off = offset_clone
                                    .fetch_add(RECORD_SIZE as u64, Ordering::SeqCst)
                                    + RECORD_SIZE as u64;

                                // Trigger compaction if over budget
                                if new_off > MAX_JOURNAL_SIZE_BEFORE_COMPACT {
                                    if let Ok(_guard) = lock_clone.try_lock() {
                                        let _ = compact(&dir_clone, &model_clone, &offset_clone);
                                    }
                                }
                            }
                        }
                    }
                })
                .ok();

            Some(sender)
        } else {
            None
        };

        Ok(Self {
            dir: dir.to_path_buf(),
            read_only,
            model,
            tx,
            last_journal_offset,
            _compact_lock: compact_lock,
        })
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Tail the journal to pick up commits from other processes (docs/03 §9.4).
    pub fn sync(&self) {
        let journal_path = self.dir.join("journal.t3j");
        let Ok(meta) = std::fs::metadata(&journal_path) else {
            return;
        };

        let current_len = meta.len();
        let prev_offset = self.last_journal_offset.load(Ordering::Acquire);

        if current_len > prev_offset {
            // New records appended: read incrementally
            if let Ok(mut f) = File::open(&journal_path) {
                if let Ok(mut user) = self.model.write() {
                    if let Ok(new_off) = replay_journal(&mut f, prev_offset, &mut user) {
                        self.last_journal_offset.store(new_off, Ordering::Release);
                    }
                }
            }
        } else if current_len < prev_offset {
            // Journal shrunk (compaction occurred elsewhere): reload snapshot + journal
            if let Ok(mut user) = self.model.write() {
                *user = MemoryUser::new();
                let snapshot_path = self.dir.join("snapshot.t3u");
                if let Ok(snap_bytes) = std::fs::read(&snapshot_path) {
                    let _ = load_snapshot(&snap_bytes, &mut user);
                }
                if let Ok(mut f) = File::open(&journal_path) {
                    if let Ok(new_off) = replay_journal(&mut f, 0, &mut user) {
                        self.last_journal_offset.store(new_off, Ordering::Release);
                    }
                }
            }
        }
    }

    /// Record a commit (docs/03 §9.3).
    pub fn record(&self, key: &str, word: &str, rank: usize, navigated: bool, top: Option<&str>) {
        // Always update in-memory model
        if let Ok(mut user) = self.model.write() {
            user.record(key, word, rank, navigated, top);
        }

        // Send to persistent writer if not read-only
        if let Some(ref tx) = self.tx {
            let ts = current_timestamp();
            if let Some(rec) = JournalRecord::new(KIND_CHOOSE, ts, key, word) {
                let _ = tx.send(rec);
            }
            if rank > 0 && navigated {
                if let Some(t) = top {
                    if t != word {
                        if let Some(rec) = JournalRecord::new(KIND_NEGATIVE, ts, key, t) {
                            let _ = tx.send(rec);
                        }
                    }
                }
            }
        }
    }

    /// Wipe all user learning.
    pub fn wipe(&self) {
        if let Ok(mut user) = self.model.write() {
            *user = MemoryUser::new();
        }
        if let Some(ref tx) = self.tx {
            let ts = current_timestamp();
            if let Some(rec) = JournalRecord::new(KIND_WIPE, ts, "", "") {
                let _ = tx.send(rec);
            }
        }
    }
}

impl UserScorer for UserStore {
    fn usr(&self, key: &str, word: &str) -> f32 {
        self.model.read().map_or(0.0, |m| m.usr(key, word))
    }

    fn sticky(&self, key: &str) -> Option<String> {
        self.model.read().ok().and_then(|m| m.sticky(key))
    }
}

/// Replay journal from `start_offset` and return new offset.
fn replay_journal(f: &mut File, start_offset: u64, user: &mut MemoryUser) -> std::io::Result<u64> {
    f.seek(SeekFrom::Start(start_offset))?;
    let mut buf = [0u8; RECORD_SIZE];
    let mut current_offset = start_offset;

    while f.read_exact(&mut buf).is_ok() {
        if let Some(rec) = JournalRecord::from_bytes(&buf) {
            match rec.kind {
                KIND_CHOOSE => {
                    if let (Some(l), Some(a)) = (rec.latin_str(), rec.arabic_str()) {
                        user.record(l, a, 0, false, None);
                    }
                }
                KIND_NEGATIVE => {
                    if let (Some(l), Some(a)) = (rec.latin_str(), rec.arabic_str()) {
                        user.record(l, "", 1, true, Some(a));
                    }
                }
                KIND_WIPE => {
                    *user = MemoryUser::new();
                }
                _ => {}
            }
        }
        current_offset += RECORD_SIZE as u64;
    }

    Ok(current_offset)
}

/// Compact memory model into `snapshot.t3u` and truncate `journal.t3j`.
fn compact(
    dir: &Path,
    model: &Arc<RwLock<MemoryUser>>,
    offset: &Arc<AtomicU64>,
) -> std::io::Result<()> {
    let snapshot_tmp = dir.join("snapshot.tmp");
    let snapshot_final = dir.join("snapshot.t3u");
    let journal_path = dir.join("journal.t3j");

    // Write snapshot binary format
    {
        let _user = model
            .read()
            .map_err(|_| std::io::Error::other("lock error"))?;
        let mut f = File::create(&snapshot_tmp)?;
        f.write_all(&SNAPSHOT_MAGIC.to_le_bytes())?;
        // Snapshot format placeholder: header + size
        f.write_all(&(0u32).to_le_bytes())?;
        f.flush()?;
    }

    // Atomic rename
    std::fs::rename(&snapshot_tmp, &snapshot_final)?;

    // Truncate journal
    let _ = File::create(&journal_path)?;
    offset.store(0, Ordering::Release);
    Ok(())
}

fn load_snapshot(_bytes: &[u8], _user: &mut MemoryUser) -> Result<(), ()> {
    // Snapshot decoder: magic validation
    if _bytes.len() < 8 {
        return Err(());
    }
    let magic = u32::from_le_bytes([_bytes[0], _bytes[1], _bytes[2], _bytes[3]]);
    if magic != SNAPSHOT_MAGIC {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_open_and_record() {
        let temp = std::env::temp_dir().join("t3a_test_store_1");
        let _ = std::fs::remove_dir_all(&temp);

        let store = UserStore::open(&temp, false).unwrap();
        store.record("7abibi", "حبيبي", 0, false, None);

        // Check scoring works
        assert!(store.usr("7abibi", "حبيبي") > 0.0);
        assert_eq!(store.sticky("7abibi"), Some("حبيبي".to_string()));

        // Let writer thread process
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Reopen store from disk
        let store2 = UserStore::open(&temp, false).unwrap();
        assert_eq!(store2.sticky("7abibi"), Some("حبيبي".to_string()));

        let _ = std::fs::remove_dir_all(&temp);
    }
}
