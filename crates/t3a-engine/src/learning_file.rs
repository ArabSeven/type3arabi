//! Learning export/import file (`.t3learn`): moves what Type3arabi learned — and, optionally, the
//! settings — to another PC or Windows account (docs/03 §9.5).
//!
//! Layout (little-endian):
//!   magic    : [u8; 8] = b"T3LEARN\0"
//!   version  : u32     = 1
//!   count    : u32     number of journal records
//!   records  : count × 128-byte journal records (crate::journal), CHOOSE / NEGATIVE only
//!   settings : u32 length + UTF-8 config.toml (length 0 = no settings)
//!
//! The records are the journal's own format, so an import is replayed exactly like a commit and every
//! running app picks it up by tailing the journal (no restart needed).

use crate::journal::{JournalRecord, KIND_CHOOSE, KIND_NEGATIVE, KIND_WIPE, RECORD_SIZE};

pub const MAGIC: &[u8; 8] = b"T3LEARN\0";
pub const VERSION: u32 = 1;
/// File extension used by the Settings app.
pub const EXTENSION: &str = "t3learn";
/// Upper bounds that keep a hostile or corrupt file from exhausting memory.
pub const MAX_RECORDS: usize = 2_000_000;
pub const MAX_SETTINGS_BYTES: usize = 256 * 1024;

/// A decoded learning file.
#[derive(Debug, Default)]
pub struct LearningFile {
    pub records: Vec<JournalRecord>,
    pub settings: Option<String>,
}

/// The learning history that matters: CHOOSE / NEGATIVE records after the last WIPE, in order.
/// Records with a bad magic or CRC (a torn write) are skipped, as journal replay does.
pub fn effective_records(journal: &[u8]) -> Vec<JournalRecord> {
    let mut out = Vec::new();
    for chunk in journal.chunks_exact(RECORD_SIZE) {
        let mut buf = [0u8; RECORD_SIZE];
        buf.copy_from_slice(chunk);
        let Some(rec) = JournalRecord::from_bytes(&buf) else {
            continue;
        };
        match rec.kind {
            KIND_WIPE => out.clear(),
            KIND_CHOOSE | KIND_NEGATIVE => out.push(rec),
            _ => {}
        }
    }
    out
}

/// Serialize records (and optional settings) into a `.t3learn` file.
pub fn encode(records: &[JournalRecord], settings: Option<&str>) -> Vec<u8> {
    let settings = settings.unwrap_or("").as_bytes();
    let mut out = Vec::with_capacity(20 + records.len() * RECORD_SIZE + settings.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for r in records {
        out.extend_from_slice(&r.to_bytes());
    }
    out.extend_from_slice(&(settings.len() as u32).to_le_bytes());
    out.extend_from_slice(settings);
    out
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "the file is truncated".to_string())
}

/// Parse and validate a `.t3learn` file. Only CHOOSE / NEGATIVE records with a valid CRC are kept.
pub fn decode(bytes: &[u8]) -> Result<LearningFile, String> {
    if bytes.len() < 20 || &bytes[..8] != MAGIC {
        return Err("not a Type3arabi learning file".into());
    }
    let version = u32_at(bytes, 8)?;
    if version != VERSION {
        return Err(format!("unsupported learning file version {version}"));
    }
    let count = u32_at(bytes, 12)? as usize;
    if count > MAX_RECORDS {
        return Err("the file is too large".into());
    }
    let start = 16;
    let end = start + count * RECORD_SIZE;
    let body = bytes
        .get(start..end)
        .ok_or_else(|| "the file is truncated".to_string())?;
    let mut records = Vec::with_capacity(count);
    for chunk in body.chunks_exact(RECORD_SIZE) {
        let mut buf = [0u8; RECORD_SIZE];
        buf.copy_from_slice(chunk);
        if let Some(rec) = JournalRecord::from_bytes(&buf) {
            if matches!(rec.kind, KIND_CHOOSE | KIND_NEGATIVE)
                && rec.latin_str().is_some()
                && rec.arabic_str().is_some()
            {
                records.push(rec);
            }
        }
    }
    let len = u32_at(bytes, end)? as usize;
    if len > MAX_SETTINGS_BYTES {
        return Err("the settings in the file are too large".into());
    }
    let raw = bytes
        .get(end + 4..end + 4 + len)
        .ok_or_else(|| "the file is truncated".to_string())?;
    let settings = if len == 0 {
        None
    } else {
        Some(
            std::str::from_utf8(raw)
                .map_err(|_| "the settings in the file are not valid text".to_string())?
                .to_string(),
        )
    };
    Ok(LearningFile { records, settings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: u8, l: &str, a: &str) -> JournalRecord {
        JournalRecord::new(kind, 1_700_000_000, l, a).unwrap()
    }

    #[test]
    fn roundtrip_with_and_without_settings() {
        let rs = vec![
            rec(KIND_CHOOSE, "7abibi", "حبيبي"), // U+062D U+0628 U+064A U+0628 U+064A
            rec(KIND_NEGATIVE, "hello", "هلو"),
        ];
        let f = decode(&encode(&rs, Some("[learning]\nenabled = true\n"))).unwrap();
        assert_eq!(f.records, rs);
        assert_eq!(f.settings.as_deref(), Some("[learning]\nenabled = true\n"));
        let f = decode(&encode(&rs, None)).unwrap();
        assert_eq!(f.records.len(), 2);
        assert!(f.settings.is_none());
    }

    #[test]
    fn history_starts_after_the_last_wipe() {
        let mut j = Vec::new();
        for r in [
            rec(KIND_CHOOSE, "a", "ا"),
            rec(KIND_WIPE, "", ""),
            rec(KIND_CHOOSE, "b", "ب"),
        ] {
            j.extend_from_slice(&r.to_bytes());
        }
        j.extend_from_slice(&[0u8; 40]); // torn tail is ignored
        let e = effective_records(&j);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].latin_str(), Some("b"));
    }

    #[test]
    fn rejects_foreign_truncated_and_corrupt_files() {
        assert!(decode(b"PK\x03\x04 not ours at all").is_err());
        let mut good = encode(&[rec(KIND_CHOOSE, "salam", "سلام")], None);
        assert!(decode(&good[..good.len() - 2]).is_err());
        // Flip a byte inside the record: its CRC fails and it is dropped, the file still parses.
        good[16 + 20] ^= 0xFF;
        assert_eq!(decode(&good).unwrap().records.len(), 0);
        // A WIPE (or any non-learning kind) smuggled into a file is not imported.
        let f = decode(&encode(&[rec(KIND_WIPE, "", "")], None)).unwrap();
        assert!(f.records.is_empty());
    }
}
