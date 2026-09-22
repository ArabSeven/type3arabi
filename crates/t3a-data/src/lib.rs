//! # t3a-data — the `type3arabi.dat` container (docs/12-binary-format.md)
//!
//! Implemented now: header + section table writer and validating reader (`DataView`), CRC32.
//! M2 adds the typed section views (TRIE, WREC, RULE, …) via `bytemuck` and the mmap loader.

use std::fmt;

pub const MAGIC: [u8; 4] = *b"T3AD";
pub const VERSION_MAJOR: u16 = 1;
pub const VERSION_MINOR: u16 = 0;
pub const HEADER_LEN: usize = 64;
pub const SECTION_ENTRY_LEN: usize = 24;

/// Section kinds (docs/12 §2).
pub mod kind {
    pub const ALPH: [u8; 4] = *b"ALPH";
    pub const TRIE: [u8; 4] = *b"TRIE";
    pub const WREC: [u8; 4] = *b"WREC";
    pub const STRS: [u8; 4] = *b"STRS";
    pub const RULE: [u8; 4] = *b"RULE";
    pub const CHNK: [u8; 4] = *b"CHNK";
    pub const BIGR: [u8; 4] = *b"BIGR";
    pub const CHLM: [u8; 4] = *b"CHLM";
    pub const PHRS: [u8; 4] = *b"PHRS";
    pub const DIAC: [u8; 4] = *b"DIAC";
    pub const REGN: [u8; 4] = *b"REGN";
    pub const PARM: [u8; 4] = *b"PARM";
    pub const META: [u8; 4] = *b"META";
    /// Required in v1 (BIGR and DIAC are optional).
    pub const REQUIRED: [[u8; 4]; 11] = [
        ALPH, TRIE, WREC, STRS, RULE, CHNK, CHLM, PHRS, REGN, PARM, META,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u16, u16),
    HeaderCrc,
    FileLen { header: u64, actual: u64 },
    SectionTable,
    SectionBounds([u8; 4]),
    SectionAlign([u8; 4]),
    MissingSection([u8; 4]),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DataError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub major: u16,
    pub minor: u16,
    pub section_count: u32,
    pub flags: u32,
    pub data_version: u64,
    pub build_id: [u8; 16],
    pub section_table_offset: u64,
    pub file_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub kind: [u8; 4],
    pub offset: u64,
    pub len: u64,
}

/// A validated, zero-copy view over a data file's bytes (usually a memory map).
#[derive(Debug, Clone, Copy)]
pub struct DataView<'a> {
    bytes: &'a [u8],
    header: Header,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// CRC-32 (IEEE, reflected) — small table-less implementation; only used on the 56-byte header at load.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

impl<'a> DataView<'a> {
    /// Validate header, section table, bounds, alignment and required sections (docs/12 §2). O(sections).
    pub fn parse(bytes: &'a [u8]) -> Result<Self, DataError> {
        if bytes.len() < HEADER_LEN {
            return Err(DataError::TooShort);
        }
        if bytes[0..4] != MAGIC {
            return Err(DataError::BadMagic);
        }
        let header = Header {
            major: u16_at(bytes, 4),
            minor: u16_at(bytes, 6),
            section_count: u32_at(bytes, 8),
            flags: u32_at(bytes, 12),
            data_version: u64_at(bytes, 16),
            build_id: bytes[24..40].try_into().unwrap(),
            section_table_offset: u64_at(bytes, 40),
            file_len: u64_at(bytes, 48),
        };
        if header.major != VERSION_MAJOR {
            return Err(DataError::UnsupportedVersion(header.major, header.minor));
        }
        if crc32(&bytes[0..56]) != u32_at(bytes, 56) {
            return Err(DataError::HeaderCrc);
        }
        if header.file_len != bytes.len() as u64 {
            return Err(DataError::FileLen {
                header: header.file_len,
                actual: bytes.len() as u64,
            });
        }
        let table_len = (header.section_count as u64)
            .checked_mul(SECTION_ENTRY_LEN as u64)
            .ok_or(DataError::SectionTable)?;
        let table_end = header
            .section_table_offset
            .checked_add(table_len)
            .ok_or(DataError::SectionTable)?;
        if header.section_table_offset < HEADER_LEN as u64 || table_end > header.file_len {
            return Err(DataError::SectionTable);
        }
        let view = DataView { bytes, header };
        for s in view.sections() {
            let end = s
                .offset
                .checked_add(s.len)
                .ok_or(DataError::SectionBounds(s.kind))?;
            if s.offset < HEADER_LEN as u64 || end > header.file_len {
                return Err(DataError::SectionBounds(s.kind));
            }
            if !s.offset.is_multiple_of(8) {
                return Err(DataError::SectionAlign(s.kind));
            }
        }
        for k in kind::REQUIRED {
            if view.section(k).is_none() {
                return Err(DataError::MissingSection(k));
            }
        }
        Ok(view)
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn sections(&self) -> impl Iterator<Item = Section> + 'a {
        let b = self.bytes;
        let base = self.header.section_table_offset as usize;
        (0..self.header.section_count as usize).map(move |i| {
            let o = base + i * SECTION_ENTRY_LEN;
            Section {
                kind: b[o..o + 4].try_into().unwrap(),
                offset: u64_at(b, o + 8),
                len: u64_at(b, o + 16),
            }
        })
    }

    /// Bytes of the first section of `kind`, if present.
    pub fn section(&self, kind: [u8; 4]) -> Option<&'a [u8]> {
        let b = self.bytes;
        self.sections()
            .find(|s| s.kind == kind)
            .map(|s| &b[s.offset as usize..(s.offset + s.len) as usize])
    }
}

/// Minimal writer: collects sections and emits a valid file (used by `t3a-cli build-data` and tests).
#[derive(Default)]
pub struct Writer {
    sections: Vec<([u8; 4], Vec<u8>)>,
    pub flags: u32,
    pub data_version: u64,
    pub build_id: [u8; 16],
}

impl Writer {
    pub fn new(data_version: u64) -> Self {
        Self {
            data_version,
            ..Default::default()
        }
    }

    pub fn add(&mut self, kind: [u8; 4], bytes: Vec<u8>) {
        self.sections.push((kind, bytes));
    }

    pub fn finish(self) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_LEN];
        let mut entries = Vec::new();
        for (k, data) in &self.sections {
            while !out.len().is_multiple_of(8) {
                out.push(0);
            }
            entries.push((*k, out.len() as u64, data.len() as u64));
            out.extend_from_slice(data);
        }
        while !out.len().is_multiple_of(8) {
            out.push(0);
        }
        let table = out.len() as u64;
        for (k, off, len) in &entries {
            out.extend_from_slice(k);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&off.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
        }
        let file_len = out.len() as u64;
        out[0..4].copy_from_slice(&MAGIC);
        out[4..6].copy_from_slice(&VERSION_MAJOR.to_le_bytes());
        out[6..8].copy_from_slice(&VERSION_MINOR.to_le_bytes());
        out[8..12].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        out[12..16].copy_from_slice(&self.flags.to_le_bytes());
        out[16..24].copy_from_slice(&self.data_version.to_le_bytes());
        out[24..40].copy_from_slice(&self.build_id);
        out[40..48].copy_from_slice(&table.to_le_bytes());
        out[48..56].copy_from_slice(&file_len.to_le_bytes());
        let crc = crc32(&out[0..56]);
        out[56..60].copy_from_slice(&crc.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> Vec<u8> {
        let mut w = Writer::new(2_026_092_201);
        for k in kind::REQUIRED {
            w.add(k, vec![1, 2, 3]);
        }
        w.finish()
    }

    #[test]
    fn roundtrip() {
        let bytes = minimal();
        let v = DataView::parse(&bytes).unwrap();
        assert_eq!(v.header().data_version, 2_026_092_201);
        assert_eq!(v.section(kind::TRIE), Some(&[1u8, 2, 3][..]));
        assert_eq!(v.section(kind::BIGR), None);
    }

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn rejects_corruption() {
        let good = minimal();
        let mut b = good.clone();
        b[0] = b'X';
        assert_eq!(DataView::parse(&b).unwrap_err(), DataError::BadMagic);
        let mut b = good.clone();
        b[20] ^= 1;
        assert_eq!(DataView::parse(&b).unwrap_err(), DataError::HeaderCrc);
        let mut b = good.clone();
        b.push(0);
        assert!(matches!(
            DataView::parse(&b).unwrap_err(),
            DataError::FileLen { .. }
        ));
        assert_eq!(
            DataView::parse(&good[..10]).unwrap_err(),
            DataError::TooShort
        );
    }

    #[test]
    fn never_panics_on_truncation_or_bit_flips() {
        // Poor man's fuzz (the cargo-fuzz target lands in M2): every truncation and single-byte flip.
        let good = minimal();
        for n in 0..good.len() {
            let _ = DataView::parse(&good[..n]);
        }
        for i in 0..good.len() {
            for bit in 0..8 {
                let mut b = good.clone();
                b[i] ^= 1 << bit;
                let _ = DataView::parse(&b);
            }
        }
    }
}
