//! Persistent, multi-process user store with journal, snapshot, tailing and compaction (docs/03 §9.4).
//!
//! Every process that loads the keyboard has its own `UserStore` over the same two files in
//! `%LOCALAPPDATA%\Type3arabi\user\`: the append-only `journal.t3j` (one 128-byte record per commit,
//! appended by each process's writer thread) and `snapshot.t3u` (the compacted model, see
//! `user::MemoryUser::encode_snapshot`). When the journal passes `COMPACT_AT_BYTES`, the writer that
//! notices merges both into a new snapshot and truncates the journal to what was appended meanwhile.
//! Compaction and appends are serialized by a lock *file* (`compact.lock`), which keeps this crate free
//! of platform APIs (R12); a stale lock (crashed process) expires after `LOCK_STALE`.

use crate::journal::{JournalRecord, KIND_CHOOSE, KIND_NEGATIVE, KIND_WIPE, RECORD_SIZE};
use crate::user::{MemoryUser, SnapshotMark, UserScorer};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const JOURNAL: &str = "journal.t3j";
const SNAPSHOT: &str = "snapshot.t3u";
const LOCK: &str = "compact.lock";
/// docs/03 §9.4: compact once the journal exceeds 256 KB (≈ 2,000 commits).
pub const COMPACT_AT_BYTES: u64 = 256 * 1024;
/// docs/03 §9.4: at most 50,000 keys survive a compaction (LRU by last use).
pub const MAX_KEYS: usize = 50_000;
/// A lock file older than this belongs to a process that died while compacting.
const LOCK_STALE: Duration = Duration::from_secs(30);
/// How long a writer waits for a running compaction before appending anyway.
const LOCK_WAIT: Duration = Duration::from_secs(2);
/// Repeats per learned choice when a compacted model is exported (`MemoryUser::to_records`).
const EXPORT_MAX_REPEAT: u32 = 16;

/// Persistent user store managing learning, journal append, and cross-process tailing.
pub struct UserStore {
    dir: PathBuf,
    read_only: bool,
    model: Arc<RwLock<MemoryUser>>,
    tx: Option<Sender<JournalRecord>>,
    /// Journal bytes already applied to `model`.
    last_journal_offset: Arc<AtomicU64>,
    /// Offsets of records this process appended (already applied in memory when committed), skipped
    /// when tailing so that other processes' records around them are neither missed nor doubled.
    own: Arc<Mutex<Vec<u64>>>,
    /// (length, mtime) of the snapshot last loaded: a change means another process compacted.
    snapshot_stamp: Mutex<Option<(u64, SystemTime)>>,
}

fn current_timestamp() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0)
}

fn snapshot_stamp(dir: &Path) -> Option<(u64, SystemTime)> {
    let m = std::fs::metadata(dir.join(SNAPSHOT)).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// Apply whole records from `bytes` (a torn trailing record is ignored, bad CRCs skipped).
fn replay_bytes(bytes: &[u8], user: &mut MemoryUser) {
    for chunk in bytes.chunks_exact(RECORD_SIZE) {
        let mut buf = [0u8; RECORD_SIZE];
        buf.copy_from_slice(chunk);
        if let Some(rec) = JournalRecord::from_bytes(&buf) {
            apply(&rec, user);
        }
    }
}

fn apply(rec: &JournalRecord, user: &mut MemoryUser) {
    match rec.kind {
        KIND_CHOOSE => {
            if let (Some(l), Some(a)) = (rec.latin_str(), rec.arabic_str()) {
                user.record(l, a, 0, false, None);
            }
        }
        KIND_NEGATIVE => {
            if let (Some(l), Some(a)) = (rec.latin_str(), rec.arabic_str()) {
                user.record_negative(l, a);
            }
        }
        KIND_WIPE => *user = MemoryUser::new(),
        _ => {}
    }
}

/// The model from the snapshot plus the journal records it does not contain yet, and the journal
/// offset reached (docs/03 §9.4; the replay rule is explained at `user::SnapshotMark`).
fn load_all(dir: &Path) -> (MemoryUser, u64) {
    let journal = std::fs::read(dir.join(JOURNAL)).unwrap_or_default();
    let aligned = journal.len() / RECORD_SIZE * RECORD_SIZE;
    let (mut user, start) = match std::fs::read(dir.join(SNAPSHOT))
        .ok()
        .and_then(|b| MemoryUser::decode_snapshot(&b))
    {
        Some((u, mark)) => {
            let still_there = mark.consumed as usize <= aligned
                && SnapshotMark::prefix_of(&journal, mark.consumed) == mark.prefix_hash;
            (
                u,
                if still_there {
                    mark.consumed as usize
                } else {
                    0
                },
            )
        }
        None => (MemoryUser::new(), 0),
    };
    replay_bytes(&journal[start..aligned], &mut user);
    (user, aligned as u64)
}

/// The compaction lock (a file created exclusively; removed on drop).
struct LockFile(PathBuf);

impl LockFile {
    fn acquire(dir: &Path) -> Option<LockFile> {
        let path = dir.join(LOCK);
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Some(LockFile(path)),
                Err(_) if lock_is_stale(&path) => {
                    let _ = std::fs::remove_file(&path);
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > LOCK_STALE)
}

/// Writers pause while another process compacts (it holds the lock for milliseconds).
fn wait_for_compaction(dir: &Path) {
    let path = dir.join(LOCK);
    let start = std::time::Instant::now();
    while path.exists() && !lock_is_stale(&path) && start.elapsed() < LOCK_WAIT {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Write `bytes` to `name` atomically (temp file in the same folder, then rename over it).
fn replace_file(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join(format!("{name}.tmp"));
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, dir.join(name))
}

/// Merge snapshot + journal into a new snapshot and truncate the journal (docs/03 §9.4). Returns
/// false when another process holds the lock. Crash-safe: the snapshot is replaced first and records
/// where the old journal ended; readers then replay correctly whether or not the journal has been
/// truncated yet.
pub fn compact(dir: &Path) -> std::io::Result<bool> {
    let Some(_lock) = LockFile::acquire(dir) else {
        return Ok(false);
    };
    let journal = std::fs::read(dir.join(JOURNAL)).unwrap_or_default();
    let aligned = journal.len() / RECORD_SIZE * RECORD_SIZE;
    let (mut user, _) = load_all(dir);
    user.evict(MAX_KEYS);
    let mark = SnapshotMark {
        consumed: aligned as u64,
        prefix_hash: SnapshotMark::prefix_of(&journal, aligned as u64),
    };
    replace_file(dir, SNAPSHOT, &user.encode_snapshot(mark))?;
    // Records appended after our read (writers that passed `wait_for_compaction` just before the lock).
    let now = std::fs::read(dir.join(JOURNAL)).unwrap_or_default();
    let tail_end = now.len() / RECORD_SIZE * RECORD_SIZE;
    let tail = now.get(aligned..tail_end).unwrap_or(&[]);
    replace_file(dir, JOURNAL, tail)?;
    Ok(true)
}

impl UserStore {
    /// Open user store at `dir`. If `read_only` is true (e.g. AppContainer or private mode),
    /// no writes or writer thread are initiated.
    pub fn open(dir: &Path, read_only: bool) -> std::io::Result<Self> {
        if !read_only {
            let _ = std::fs::create_dir_all(dir);
        }
        let stamp = snapshot_stamp(dir);
        let (user, last_offset) = load_all(dir);
        let model = Arc::new(RwLock::new(user));
        let last_journal_offset = Arc::new(AtomicU64::new(last_offset));
        let own: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

        // Background writer thread if writable (R4: the store itself is opened lazily).
        let tx = if !read_only {
            let (sender, receiver) = channel::<JournalRecord>();
            let dir2 = dir.to_path_buf();
            let own2 = Arc::clone(&own);
            std::thread::Builder::new()
                .name("t3a-user-writer".into())
                .spawn(move || {
                    let j_path = dir2.join(JOURNAL);
                    while let Ok(rec) = receiver.recv() {
                        wait_for_compaction(&dir2);
                        let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&j_path)
                        else {
                            continue;
                        };
                        if f.write_all(&rec.to_bytes()).is_err() {
                            continue;
                        }
                        let _ = f.flush();
                        // An append lands at the end of the file: our handle's position is its end.
                        if let Ok(end) = f.stream_position() {
                            if let Ok(mut o) = own2.lock() {
                                o.push(end.saturating_sub(RECORD_SIZE as u64));
                            }
                            if end > COMPACT_AT_BYTES {
                                drop(f);
                                let _ = compact(&dir2);
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
            own,
            snapshot_stamp: Mutex::new(stamp),
        })
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Tail the journal to pick up commits from other processes (docs/03 §9.4). A new snapshot (someone
    /// compacted) or a shorter journal means a full reload.
    pub fn sync(&self) {
        let journal_path = self.dir.join(JOURNAL);
        let current_len = std::fs::metadata(&journal_path).map_or(0, |m| m.len());
        let prev_offset = self.last_journal_offset.load(Ordering::Acquire);
        let stamp = snapshot_stamp(&self.dir);
        let snapshot_changed = self.snapshot_stamp.lock().is_ok_and(|s| *s != stamp);

        if snapshot_changed || current_len < prev_offset {
            let (user, offset) = load_all(&self.dir);
            if let Ok(mut m) = self.model.write() {
                *m = user;
            }
            self.last_journal_offset.store(offset, Ordering::Release);
            if let Ok(mut s) = self.snapshot_stamp.lock() {
                *s = stamp;
            }
            if let Ok(mut o) = self.own.lock() {
                // Our records up to `offset` are in the reloaded model now.
                o.retain(|&p| p >= offset);
            }
            return;
        }
        if current_len >= prev_offset + RECORD_SIZE as u64 {
            let Ok(mut f) = File::open(&journal_path) else {
                return;
            };
            if f.seek(SeekFrom::Start(prev_offset)).is_err() {
                return;
            }
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_err() {
                return;
            }
            let whole = buf.len() / RECORD_SIZE * RECORD_SIZE;
            let mut own = self.own.lock().ok();
            if let Ok(mut user) = self.model.write() {
                for (i, chunk) in buf[..whole].chunks_exact(RECORD_SIZE).enumerate() {
                    let at = prev_offset + (i * RECORD_SIZE) as u64;
                    if let Some(o) = own.as_mut() {
                        if let Some(pos) = o.iter().position(|&p| p == at) {
                            o.swap_remove(pos);
                            continue; // applied when it was committed here
                        }
                    }
                    let mut rec = [0u8; RECORD_SIZE];
                    rec.copy_from_slice(chunk);
                    if let Some(rec) = JournalRecord::from_bytes(&rec) {
                        apply(&rec, &mut user);
                    }
                }
            }
            self.last_journal_offset
                .store(prev_offset + whole as u64, Ordering::Release);
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

    /// Export what was learned (everything since the last wipe), plus optional settings, as a
    /// `.t3learn` file (docs/03 §9.5). Before the first compaction this is the journal's own history;
    /// afterwards the compacted model is written out as records (`MemoryUser::to_records`).
    pub fn export(&self, settings: Option<&str>) -> std::io::Result<Vec<u8>> {
        let journal = match std::fs::read(self.dir.join(JOURNAL)) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let records = if self.dir.join(SNAPSHOT).exists() {
            load_all(&self.dir)
                .0
                .to_records(current_timestamp(), EXPORT_MAX_REPEAT)
        } else {
            crate::learning_file::effective_records(&journal)
        };
        Ok(crate::learning_file::encode(&records, settings))
    }

    /// Import learned choices from a decoded `.t3learn` file. `replace` forgets the current learning
    /// first; otherwise the file is merged in. Records go through the journal like normal commits, so
    /// every running app picks them up. Returns the number of records imported.
    pub fn import(
        &self,
        file: &crate::learning_file::LearningFile,
        replace: bool,
    ) -> std::io::Result<usize> {
        if self.tx.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the learning store is read-only here",
            ));
        }
        if replace {
            self.wipe();
        }
        let mut n = 0;
        for rec in &file.records {
            let (Some(latin), Some(arabic)) = (rec.latin_str(), rec.arabic_str()) else {
                continue;
            };
            if let Ok(mut user) = self.model.write() {
                match rec.kind {
                    KIND_CHOOSE => user.record(latin, arabic, 0, false, None),
                    KIND_NEGATIVE => user.record_negative(latin, arabic),
                    _ => continue,
                }
            }
            if let Some(ref tx) = self.tx {
                let _ = tx.send(*rec);
            }
            n += 1;
        }
        Ok(n)
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

    /// Regression: a NEGATIVE journal record replayed as "user chose the empty string", which
    /// surfaced as a blank candidate that crashed the popup (DrawTextW on an empty buffer).
    #[test]
    fn export_then_import_moves_learning_to_another_store() {
        let a = std::env::temp_dir().join("t3a_test_store_export_a");
        let b = std::env::temp_dir().join("t3a_test_store_export_b");
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
        let src = UserStore::open(&a, false).unwrap();
        src.record("aghati", "أغاتي", 0, false, None); // U+0623 U+063A U+0627 U+062A U+064A
        src.wipe();
        src.record("shloonak", "شلونك", 1, true, Some("شلوونك"));
        std::thread::sleep(std::time::Duration::from_millis(150));
        let file = src.export(Some("[dialect]\nprofile = \"auto\"\n")).unwrap();
        drop(src);

        let parsed = crate::learning_file::decode(&file).unwrap();
        assert_eq!(parsed.records.len(), 2); // choice + negative; the pre-wipe choice is gone
        let dst = UserStore::open(&b, false).unwrap();
        dst.record("other", "غير", 0, false, None);
        assert_eq!(dst.import(&parsed, true).unwrap(), 2);
        assert_eq!(dst.sticky("shloonak").as_deref(), Some("شلونك"));
        assert!(dst.sticky("other").is_none()); // replace mode forgot the old learning
        std::thread::sleep(std::time::Duration::from_millis(150));
        drop(dst);
        // Persisted through the journal: a fresh reader sees the imported choice.
        let reopened = UserStore::open(&b, true).unwrap();
        assert_eq!(reopened.sticky("shloonak").as_deref(), Some("شلونك"));
        assert!(reopened.sticky("aghati").is_none());
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn navigated_choice_survives_reopen_without_empty_word() {
        let temp = std::env::temp_dir().join("t3a_test_store_negative");
        let _ = std::fs::remove_dir_all(&temp);
        let store = UserStore::open(&temp, false).unwrap();
        // User navigated to rank 2 (raw Latin) past the rank-1 word.
        store.record("hello", crate::user::RAW_LATIN, 2, true, Some("هله"));
        std::thread::sleep(std::time::Duration::from_millis(100));
        drop(store);

        let reopened = UserStore::open(&temp, true).unwrap();
        assert_eq!(
            reopened.sticky("hello").as_deref(),
            Some(crate::user::RAW_LATIN)
        );
        assert!(reopened.usr("hello", "هله") < 0.0); // negative evidence kept
        assert_eq!(reopened.usr("hello", ""), 0.0); // no empty word learned
        let _ = std::fs::remove_dir_all(&temp);
    }

    fn fresh(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("t3a_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn wait_for_writer() {
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    /// docs/03 §9.4: past 256 KB the journal is merged into the snapshot and truncated; nothing learned
    /// is lost, and a fresh reader (another app starting) sees the same choices.
    #[test]
    fn compaction_keeps_learning_and_bounds_the_journal() {
        let dir = fresh("compact");
        let store = UserStore::open(&dir, false).unwrap();
        let n = (COMPACT_AT_BYTES as usize / RECORD_SIZE) + 50;
        for i in 0..n {
            store.record(&format!("w{}", i % 300), "كلمة", 0, false, None); // U+0643 U+0644 U+0645 U+0629
        }
        store.record("7abibi", "حبيبي", 0, false, None); // U+062D U+0628 U+064A U+0628 U+064A
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let journal = std::fs::metadata(dir.join(JOURNAL)).unwrap().len();
        assert!(
            journal < COMPACT_AT_BYTES,
            "journal was not compacted: {journal} bytes"
        );
        assert!(dir.join(SNAPSHOT).exists());
        let reader = UserStore::open(&dir, true).unwrap();
        assert_eq!(reader.sticky("7abibi").as_deref(), Some("حبيبي"));
        assert_eq!(reader.sticky("w299").as_deref(), Some("كلمة"));
        // Counts survive, not just presence.
        assert_eq!(reader.usr("w7", "كلمة"), store.usr("w7", "كلمة"));
        // An export after compaction still carries the learning.
        let parsed = crate::learning_file::decode(&store.export(None).unwrap()).unwrap();
        assert!(parsed
            .records
            .iter()
            .any(|r| r.latin_str() == Some("7abibi")));
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A compaction interrupted between writing the snapshot and truncating the journal (or a reader
    /// in between) must neither lose nor double-count records.
    #[test]
    fn interrupted_compaction_neither_loses_nor_doubles() {
        let dir = fresh("interrupted");
        let rec = |k: &str| {
            JournalRecord::new(KIND_CHOOSE, 1, k, "كلمة")
                .unwrap()
                .to_bytes()
        };
        let mut journal = Vec::new();
        for k in ["a", "b", "a"] {
            journal.extend_from_slice(&rec(k));
        }
        std::fs::write(dir.join(JOURNAL), &journal).unwrap();
        let (merged, _) = load_all(&dir);
        let mark = SnapshotMark {
            consumed: journal.len() as u64,
            prefix_hash: SnapshotMark::prefix_of(&journal, journal.len() as u64),
        };
        std::fs::write(dir.join(SNAPSHOT), merged.encode_snapshot(mark)).unwrap();
        // One more commit arrived; the journal is not truncated yet.
        journal.extend_from_slice(&rec("c"));
        std::fs::write(dir.join(JOURNAL), &journal).unwrap();
        let (before, _) = load_all(&dir);
        // Now truncated to the tail.
        std::fs::write(dir.join(JOURNAL), rec("c")).unwrap();
        let (after, _) = load_all(&dir);
        let mut expected = MemoryUser::new();
        for k in ["a", "b", "a", "c"] {
            expected.record(k, "كلمة", 0, false, None);
        }
        for u in [&before, &after] {
            for k in ["a", "b", "c"] {
                assert_eq!(u.usr(k, "كلمة"), expected.usr(k, "كلمة"), "{k}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Two apps learning at the same time: each sees the other's commits exactly once, whatever the
    /// interleaving of their appends (a store used to assume its own record sat at its read offset).
    #[test]
    fn two_processes_see_each_others_commits_once() {
        let dir = fresh("two");
        let a = UserStore::open(&dir, false).unwrap();
        let b = UserStore::open(&dir, false).unwrap();
        b.record("ahlan", "أهلا", 0, false, None); // U+0623 U+0647 U+0644 U+0627
        wait_for_writer();
        a.record("shukran", "شكرا", 0, false, None); // U+0634 U+0643 U+0631 U+0627
        wait_for_writer();
        a.sync();
        b.sync();
        let single = {
            let mut u = MemoryUser::new();
            u.record("ahlan", "أهلا", 0, false, None);
            u.record("shukran", "شكرا", 0, false, None);
            u
        };
        for s in [&a, &b] {
            assert_eq!(s.usr("ahlan", "أهلا"), single.usr("ahlan", "أهلا"));
            assert_eq!(s.usr("shukran", "شكرا"), single.usr("shukran", "شكرا"));
        }
        drop((a, b));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
