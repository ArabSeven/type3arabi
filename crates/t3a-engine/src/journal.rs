//! Journal record format and serialization for the user store (docs/03 §9.4).
//!
//! Fixed 128-byte binary record layout:
//!   magic      : u16 = 0x5433 ('T3')
//!   ver        : u8  = 1
//!   kind       : u8  (1 Choose, 2 Negative, 3 AddWord, 4 DeleteWord, 5 Dialect, 6 Wipe)
//!   ts         : u32 (Unix timestamp seconds)
//!   latin_len  : u8  (≤ 32)
//!   arabic_len : u8  (≤ 84)
//!   latin      : [u8; 32] (UTF-8, ASCII normalized)
//!   arabic     : [u8; 84] (UTF-8 base Arabic text)
//!   crc16      : u16 (CRC-16/CCITT over bytes 0..126)

pub const JOURNAL_MAGIC: u16 = 0x5433;
pub const JOURNAL_VERSION: u8 = 1;
pub const RECORD_SIZE: usize = 128;

pub const KIND_CHOOSE: u8 = 1;
pub const KIND_NEGATIVE: u8 = 2;
pub const KIND_ADD_WORD: u8 = 3;
pub const KIND_DELETE_WORD: u8 = 4;
pub const KIND_DIALECT: u8 = 5;
pub const KIND_WIPE: u8 = 6;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalRecord {
    pub magic: u16,
    pub ver: u8,
    pub kind: u8,
    pub ts: u32,
    pub latin_len: u8,
    pub arabic_len: u8,
    pub latin: [u8; 32],
    pub arabic: [u8; 84],
    pub crc16: u16,
}

// SAFETY: All fields are integer types and fixed arrays with no padding holes.
// Total size = 2 + 1 + 1 + 4 + 1 + 1 + 32 + 84 + 2 = 128 bytes.
const _: () = assert!(std::mem::size_of::<JournalRecord>() == RECORD_SIZE);

// CRC-16/CCITT (poly 0x1021, init 0xFFFF)
pub fn crc16_ccitt(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in bytes {
        let mut x = ((crc >> 8) ^ (b as u16)) & 0xFF;
        x ^= x >> 4;
        crc = (crc << 8) ^ (x << 12) ^ (x << 5) ^ x;
    }
    crc
}

impl JournalRecord {
    pub fn new(kind: u8, ts: u32, latin: &str, arabic: &str) -> Option<Self> {
        let l_bytes = latin.as_bytes();
        let a_bytes = arabic.as_bytes();
        if l_bytes.len() > 32 || a_bytes.len() > 84 {
            return None;
        }

        let mut lat_arr = [0u8; 32];
        lat_arr[..l_bytes.len()].copy_from_slice(l_bytes);

        let mut ar_arr = [0u8; 84];
        ar_arr[..a_bytes.len()].copy_from_slice(a_bytes);

        let mut rec = Self {
            magic: JOURNAL_MAGIC,
            ver: JOURNAL_VERSION,
            kind,
            ts,
            latin_len: l_bytes.len() as u8,
            arabic_len: a_bytes.len() as u8,
            latin: lat_arr,
            arabic: ar_arr,
            crc16: 0,
        };

        // Compute CRC over first 126 bytes
        let raw = rec.to_bytes();
        rec.crc16 = crc16_ccitt(&raw[..126]);
        Some(rec)
    }

    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut buf = [0u8; RECORD_SIZE];
        buf[0..2].copy_from_slice(&self.magic.to_le_bytes());
        buf[2] = self.ver;
        buf[3] = self.kind;
        buf[4..8].copy_from_slice(&self.ts.to_le_bytes());
        buf[8] = self.latin_len;
        buf[9] = self.arabic_len;
        buf[10..42].copy_from_slice(&self.latin);
        buf[42..126].copy_from_slice(&self.arabic);
        buf[126..128].copy_from_slice(&self.crc16.to_le_bytes());
        buf
    }

    pub fn from_bytes(buf: &[u8; RECORD_SIZE]) -> Option<Self> {
        let magic = u16::from_le_bytes([buf[0], buf[1]]);
        let ver = buf[2];
        if magic != JOURNAL_MAGIC || ver != JOURNAL_VERSION {
            return None;
        }

        let kind = buf[3];
        let ts = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let latin_len = buf[8];
        let arabic_len = buf[9];
        if latin_len > 32 || arabic_len > 84 {
            return None;
        }

        let mut latin = [0u8; 32];
        latin.copy_from_slice(&buf[10..42]);

        let mut arabic = [0u8; 84];
        arabic.copy_from_slice(&buf[42..126]);

        let crc16 = u16::from_le_bytes([buf[126], buf[127]]);
        let expected_crc = crc16_ccitt(&buf[..126]);
        if crc16 != expected_crc {
            return None;
        }

        Some(Self {
            magic,
            ver,
            kind,
            ts,
            latin_len,
            arabic_len,
            latin,
            arabic,
            crc16,
        })
    }

    pub fn latin_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.latin[..self.latin_len as usize]).ok()
    }

    pub fn arabic_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.arabic[..self.arabic_len as usize]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_record() {
        let rec = JournalRecord::new(KIND_CHOOSE, 1727049600, "7abibi", "حبيبي").unwrap();
        let bytes = rec.to_bytes();
        let parsed = JournalRecord::from_bytes(&bytes).unwrap();
        assert_eq!(rec, parsed);
        assert_eq!(parsed.latin_str(), Some("7abibi"));
        assert_eq!(parsed.arabic_str(), Some("حبيبي"));
    }

    #[test]
    fn detects_crc_corruption() {
        let rec = JournalRecord::new(KIND_CHOOSE, 1727049600, "7abibi", "حبيبي").unwrap();
        let mut bytes = rec.to_bytes();
        bytes[20] ^= 0xFF; // corrupt one byte
        assert!(JournalRecord::from_bytes(&bytes).is_none());
    }

    #[test]
    fn rejects_invalid_magic_or_version() {
        let rec = JournalRecord::new(KIND_CHOOSE, 1727049600, "7abibi", "حبيبي").unwrap();
        let mut bytes = rec.to_bytes();
        bytes[0] = 0x00;
        assert!(JournalRecord::from_bytes(&bytes).is_none());
    }
}
