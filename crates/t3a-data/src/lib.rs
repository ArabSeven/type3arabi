//! # t3a-data — the `type3arabi.dat` container (docs/12-binary-format.md)
//!
//! Provides zero-copy validated reading (`DataView`), typed section views via `bytemuck`,
//! memory-mapped file loading (`DataFile`), and data builder packaging (`Writer`).

pub mod records;
pub use records::*;

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
    CorruptSection([u8; 4]),
    BadStringOffset,
    BadUtf8,
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

/// View of the optional `BIGR` section.
#[derive(Clone, Copy, Debug)]
pub struct BigramView<'a> {
    pub offsets: &'a [u32],
    pub pairs: &'a [BigramPair],
}

impl<'a> BigramView<'a> {
    pub fn successors_of(&self, word_idx: usize) -> &'a [BigramPair] {
        if word_idx + 1 >= self.offsets.len() {
            return &[];
        }
        let start = self.offsets[word_idx] as usize;
        let end = self.offsets[word_idx + 1] as usize;
        if start <= end && end <= self.pairs.len() {
            &self.pairs[start..end]
        } else {
            &[]
        }
    }
}

/// View of the optional `DIAC` section.
#[derive(Clone, Copy, Debug)]
pub struct DiacView<'a> {
    pub headers: &'a [DiacHeader],
    pub variants: &'a [DiacVariant],
}

impl<'a> DiacView<'a> {
    pub fn variants_for(&self, diac_idx: usize) -> &'a [DiacVariant] {
        if diac_idx >= self.headers.len() {
            return &[];
        }
        let h = self.headers[diac_idx];
        let start = h.first as usize;
        let end = start + h.n as usize;
        if end <= self.variants.len() {
            &self.variants[start..end]
        } else {
            &[]
        }
    }
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

    /// T3A alphabet Unicode codepoints (docs/12 §3).
    pub fn alphabet(&self) -> Result<&'a [u32], DataError> {
        let raw = self
            .section(kind::ALPH)
            .ok_or(DataError::MissingSection(kind::ALPH))?;
        if raw.len() < 4 {
            return Err(DataError::CorruptSection(kind::ALPH));
        }
        let count = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[4..])
            .map_err(|_| DataError::CorruptSection(kind::ALPH))?;
        if slice.len() != count {
            return Err(DataError::CorruptSection(kind::ALPH));
        }
        Ok(slice)
    }

    /// Lexicon trie nodes in BFS order (docs/12 §4).
    pub fn trie_nodes(&self) -> Result<&'a [Node], DataError> {
        let raw = self
            .section(kind::TRIE)
            .ok_or(DataError::MissingSection(kind::TRIE))?;
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::TRIE));
        }
        let count = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[8..])
            .map_err(|_| DataError::CorruptSection(kind::TRIE))?;
        if slice.len() != count {
            return Err(DataError::CorruptSection(kind::TRIE));
        }
        Ok(slice)
    }

    /// Word records (docs/12 §4).
    pub fn words(&self) -> Result<&'a [WordRec], DataError> {
        let raw = self
            .section(kind::WREC)
            .ok_or(DataError::MissingSection(kind::WREC))?;
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::WREC));
        }
        let count = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[8..])
            .map_err(|_| DataError::CorruptSection(kind::WREC))?;
        if slice.len() != count {
            return Err(DataError::CorruptSection(kind::WREC));
        }
        Ok(slice)
    }

    /// Transliteration rules (docs/12 §5).
    pub fn rules(&self) -> Result<&'a [Rule], DataError> {
        let raw = self
            .section(kind::RULE)
            .ok_or(DataError::MissingSection(kind::RULE))?;
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::RULE));
        }
        let count = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[8..])
            .map_err(|_| DataError::CorruptSection(kind::RULE))?;
        if slice.len() != count {
            return Err(DataError::CorruptSection(kind::RULE));
        }
        Ok(slice)
    }

    /// Latin chunk index (docs/12 §5).
    pub fn chunks(&self) -> Result<&'a [Chunk], DataError> {
        let raw = self
            .section(kind::CHNK)
            .ok_or(DataError::MissingSection(kind::CHNK))?;
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::CHNK));
        }
        let count = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[8..])
            .map_err(|_| DataError::CorruptSection(kind::CHNK))?;
        if slice.len() != count {
            return Err(DataError::CorruptSection(kind::CHNK));
        }
        Ok(slice)
    }

    /// Character LM hash table (docs/12 §7).
    pub fn chlm(&self) -> Result<&'a [ChlmEntry], DataError> {
        let raw = self
            .section(kind::CHLM)
            .ok_or(DataError::MissingSection(kind::CHLM))?;
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::CHLM));
        }
        let capacity = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[8..])
            .map_err(|_| DataError::CorruptSection(kind::CHLM))?;
        if slice.len() != capacity {
            return Err(DataError::CorruptSection(kind::CHLM));
        }
        Ok(slice)
    }

    /// Phrase records (docs/12 §8).
    pub fn phrases(&self) -> Result<&'a [PhraseEntry], DataError> {
        let raw = self
            .section(kind::PHRS)
            .ok_or(DataError::MissingSection(kind::PHRS))?;
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::PHRS));
        }
        let count = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[8..])
            .map_err(|_| DataError::CorruptSection(kind::PHRS))?;
        if slice.len() != count {
            return Err(DataError::CorruptSection(kind::PHRS));
        }
        Ok(slice)
    }

    /// Region dialect priors (docs/12 §10).
    pub fn regions(&self) -> Result<&'a [RegionEntry], DataError> {
        let raw = self
            .section(kind::REGN)
            .ok_or(DataError::MissingSection(kind::REGN))?;
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::REGN));
        }
        let count = u32_at(raw, 0) as usize;
        let slice = bytemuck::try_cast_slice(&raw[8..])
            .map_err(|_| DataError::CorruptSection(kind::REGN))?;
        if slice.len() != count {
            return Err(DataError::CorruptSection(kind::REGN));
        }
        Ok(slice)
    }

    /// Length-prefixed UTF-8 string at byte `offset` in `STRS` (docs/12 §4).
    pub fn string(&self, offset: u32) -> Result<&'a str, DataError> {
        let strs = self
            .section(kind::STRS)
            .ok_or(DataError::MissingSection(kind::STRS))?;
        let off = offset as usize;
        if off >= strs.len() {
            return Err(DataError::BadStringOffset);
        }
        let len = strs[off] as usize;
        if off + 1 + len > strs.len() {
            return Err(DataError::BadStringOffset);
        }
        std::str::from_utf8(&strs[off + 1..off + 1 + len]).map_err(|_| DataError::BadUtf8)
    }

    /// Engine parameters TOML string (docs/12 §10).
    pub fn parm(&self) -> Result<&'a str, DataError> {
        let raw = self
            .section(kind::PARM)
            .ok_or(DataError::MissingSection(kind::PARM))?;
        std::str::from_utf8(raw).map_err(|_| DataError::BadUtf8)
    }

    /// Metadata JSON string (docs/12 §10).
    pub fn meta(&self) -> Result<&'a str, DataError> {
        let raw = self
            .section(kind::META)
            .ok_or(DataError::MissingSection(kind::META))?;
        std::str::from_utf8(raw).map_err(|_| DataError::BadUtf8)
    }

    /// Bigrams view (docs/12 §6, optional section).
    pub fn bigrams(&self) -> Result<Option<BigramView<'a>>, DataError> {
        let Some(raw) = self.section(kind::BIGR) else {
            return Ok(None);
        };
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::BIGR));
        }
        let prev_count = u32_at(raw, 0) as usize;
        let pair_count = u32_at(raw, 4) as usize;

        let offsets_bytes_len = (prev_count + 1) * 4;
        if 8 + offsets_bytes_len > raw.len() {
            return Err(DataError::CorruptSection(kind::BIGR));
        }
        let offsets: &[u32] = bytemuck::try_cast_slice(&raw[8..8 + offsets_bytes_len])
            .map_err(|_| DataError::CorruptSection(kind::BIGR))?;

        let pairs_start = 8 + offsets_bytes_len;
        let pairs: &[BigramPair] = bytemuck::try_cast_slice(&raw[pairs_start..])
            .map_err(|_| DataError::CorruptSection(kind::BIGR))?;
        if pairs.len() != pair_count {
            return Err(DataError::CorruptSection(kind::BIGR));
        }

        Ok(Some(BigramView { offsets, pairs }))
    }

    /// Vocalization variants view (docs/12 §9, optional section).
    pub fn diacritics(&self) -> Result<Option<DiacView<'a>>, DataError> {
        let Some(raw) = self.section(kind::DIAC) else {
            return Ok(None);
        };
        if raw.len() < 8 {
            return Err(DataError::CorruptSection(kind::DIAC));
        }
        let count = u32_at(raw, 0) as usize;
        let headers_bytes_len = count * std::mem::size_of::<DiacHeader>();
        if 8 + headers_bytes_len + 8 > raw.len() {
            return Err(DataError::CorruptSection(kind::DIAC));
        }
        let headers: &[DiacHeader] = bytemuck::try_cast_slice(&raw[8..8 + headers_bytes_len])
            .map_err(|_| DataError::CorruptSection(kind::DIAC))?;

        let variants_header_off = 8 + headers_bytes_len;
        let total_variants = u32_at(raw, variants_header_off) as usize;
        let variants_start = variants_header_off + 8;
        let variants: &[DiacVariant] = bytemuck::try_cast_slice(&raw[variants_start..])
            .map_err(|_| DataError::CorruptSection(kind::DIAC))?;
        if variants.len() != total_variants {
            return Err(DataError::CorruptSection(kind::DIAC));
        }

        Ok(Some(DiacView { headers, variants }))
    }
}

/// Memory-mapped file loader for `type3arabi.dat`.
#[cfg(feature = "mmap")]
pub struct DataFile {
    mmap: memmap2::Mmap,
}

#[cfg(feature = "mmap")]
impl DataFile {
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        // SAFETY: File is opened read-only and mapped read-only for zero-copy inspection.
        let mmap = unsafe { memmap2::Mmap::map(&file)? };
        Ok(Self { mmap })
    }

    pub fn view(&self) -> Result<DataView<'_>, DataError> {
        DataView::parse(&self.mmap)
    }
}

/// Writer: builds `type3arabi.dat` binary format with all section types (used by `t3a-cli build-data` and tests).
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

    pub fn add_alph(&mut self, codepoints: &[u32]) {
        let mut bytes = Vec::with_capacity(4 + codepoints.len() * 4);
        bytes.extend_from_slice(&(codepoints.len() as u32).to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(codepoints));
        self.add(kind::ALPH, bytes);
    }

    pub fn add_trie(&mut self, nodes: &[Node]) {
        let mut bytes = Vec::with_capacity(8 + std::mem::size_of_val(nodes));
        bytes.extend_from_slice(&(nodes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // pad
        bytes.extend_from_slice(bytemuck::cast_slice(nodes));
        self.add(kind::TRIE, bytes);
    }

    pub fn add_words(&mut self, words: &[WordRec]) {
        let mut bytes = Vec::with_capacity(8 + std::mem::size_of_val(words));
        bytes.extend_from_slice(&(words.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(words));
        self.add(kind::WREC, bytes);
    }

    pub fn add_rules(&mut self, rules: &[Rule]) {
        let mut bytes = Vec::with_capacity(8 + std::mem::size_of_val(rules));
        bytes.extend_from_slice(&(rules.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(rules));
        self.add(kind::RULE, bytes);
    }

    pub fn add_chunks(&mut self, chunks: &[Chunk]) {
        let mut bytes = Vec::with_capacity(8 + std::mem::size_of_val(chunks));
        bytes.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(chunks));
        self.add(kind::CHNK, bytes);
    }

    pub fn add_chlm(&mut self, entries: &[ChlmEntry]) {
        let mut bytes = Vec::with_capacity(8 + std::mem::size_of_val(entries));
        bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(entries));
        self.add(kind::CHLM, bytes);
    }

    pub fn add_phrases(&mut self, phrases: &[PhraseEntry]) {
        let mut bytes = Vec::with_capacity(8 + std::mem::size_of_val(phrases));
        bytes.extend_from_slice(&(phrases.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(phrases));
        self.add(kind::PHRS, bytes);
    }

    pub fn add_regions(&mut self, regions: &[RegionEntry]) {
        let mut bytes = Vec::with_capacity(8 + std::mem::size_of_val(regions));
        bytes.extend_from_slice(&(regions.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(regions));
        self.add(kind::REGN, bytes);
    }

    pub fn add_strs(&mut self, bytes: Vec<u8>) {
        self.add(kind::STRS, bytes);
    }

    pub fn add_parm(&mut self, toml_str: &str) {
        self.add(kind::PARM, toml_str.as_bytes().to_vec());
    }

    pub fn add_meta(&mut self, json_str: &str) {
        self.add(kind::META, json_str.as_bytes().to_vec());
    }

    pub fn add_bigrams(&mut self, offsets: &[u32], pairs: &[BigramPair]) {
        let mut bytes = Vec::new();
        let prev_count = offsets.len().saturating_sub(1) as u32;
        bytes.extend_from_slice(&prev_count.to_le_bytes());
        bytes.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(offsets));
        bytes.extend_from_slice(bytemuck::cast_slice(pairs));
        self.flags |= 2; // bit1 = has BIGR
        self.add(kind::BIGR, bytes);
    }

    pub fn add_diac(&mut self, headers: &[DiacHeader], variants: &[DiacVariant]) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(headers.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(headers));
        bytes.extend_from_slice(&(variants.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(bytemuck::cast_slice(variants));
        self.flags |= 1; // bit0 = has DIAC
        self.add(kind::DIAC, bytes);
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

/// Helper for packing length-prefixed strings into a STRS buffer.
#[derive(Default)]
pub struct StringPool {
    bytes: Vec<u8>,
}

impl StringPool {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Add a string and return its byte offset in the STRS buffer.
    pub fn add(&mut self, s: &str) -> u32 {
        let offset = self.bytes.len() as u32;
        let s_bytes = s.as_bytes();
        let len = s_bytes.len().min(255) as u8;
        self.bytes.push(len);
        self.bytes.extend_from_slice(&s_bytes[..len as usize]);
        offset
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Quantize natural log-probability to u8.
/// lp = -q / 8.0, so q = (-lp * 8.0).round().
/// Clamped to 0..=254. If lp is not finite or <= -31.75, returns 255 (absent).
pub fn quantize_lp(lp: f32) -> u8 {
    if !lp.is_finite() || lp <= -31.75 {
        255
    } else {
        let q = (-lp * 8.0).round();
        if q < 0.0 {
            0
        } else if q > 254.0 {
            254
        } else {
            q as u8
        }
    }
}

/// Dequantize u8 to natural log-probability.
/// lp = -q / 8.0. If q == 255, returns f32::NEG_INFINITY.
pub fn dequantize_lp(q: u8) -> f32 {
    if q == 255 {
        f32::NEG_INFINITY
    } else {
        -(q as f32) / 8.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data() -> Vec<u8> {
        let mut w = Writer::new(2_026_092_301);
        let mut strs = StringPool::new();
        let s_hub = strs.add("حب");
        let s_habibi = strs.add("حبيبي");

        w.add_alph(&[0x0621, 0x0627, 0x0628, 0x062D]);
        w.add_trie(&[
            Node {
                first_child: 1,
                word: 0,
                label: 0,
                child_count: 1,
                max_q: 10,
                flags: 0,
            },
            Node {
                first_child: 0,
                word: 1,
                label: 13,
                child_count: 0,
                max_q: 10,
                flags: 0,
            },
        ]);
        w.add_words(&[
            WordRec {
                q: [10, 12, 11, 10, 10, 10],
                flags: 0,
                surface: s_hub,
                diac: 0,
            },
            WordRec {
                q: [5, 4, 3, 5, 5, 5],
                flags: 0,
                surface: s_habibi,
                diac: 0,
            },
        ]);
        w.add_rules(&[Rule {
            arabic: [13, 0, 0],
            arabic_len: 1,
            pos_mask: 7,
            flags: 0,
            q_any: 0,
            _pad: 0,
            q: [0; 6],
            chunk: 0,
        }]);
        w.add_chunks(&[Chunk {
            latin: [b'7', 0, 0, 0],
            len: 1,
            pad: 0,
            first_rule: 0,
            rule_count: 1,
            pad2: 0,
        }]);
        w.add_chlm(&[ChlmEntry {
            fp: 12345,
            q: 20,
            order: 2,
            pad: 0,
        }]);
        w.add_phrases(&[PhraseEntry {
            key: 0,
            out: s_hub,
            dialect_mask: 0xFF,
            flags: 1,
            pad: 0,
        }]);
        w.add_regions(&[RegionEntry {
            iso2: *b"SA",
            pad: [0; 2],
            weights: [0.35, 0.15, 0.15, 0.25, 0.05, 0.05],
        }]);
        w.add_strs(strs.finish());
        w.add_parm("lambda_tm = 1.0\n");
        w.add_meta("{\"version\": 1}\n");

        w.finish()
    }

    #[test]
    fn roundtrip_typed_views() {
        let bytes = sample_data();
        let v = DataView::parse(&bytes).unwrap();

        assert_eq!(v.header().data_version, 2_026_092_301);
        assert_eq!(v.alphabet().unwrap().len(), 4);
        assert_eq!(v.trie_nodes().unwrap().len(), 2);
        assert_eq!(v.words().unwrap().len(), 2);
        assert_eq!(v.rules().unwrap().len(), 1);
        assert_eq!(v.chunks().unwrap().len(), 1);
        assert_eq!(v.chlm().unwrap().len(), 1);
        assert_eq!(v.phrases().unwrap().len(), 1);
        assert_eq!(v.regions().unwrap().len(), 1);

        assert_eq!(v.string(0).unwrap(), "حب");
        assert_eq!(v.parm().unwrap(), "lambda_tm = 1.0\n");
        assert_eq!(v.meta().unwrap(), "{\"version\": 1}\n");
    }

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn rejects_corruption() {
        let good = sample_data();
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
    }

    #[test]
    fn quantize_roundtrip() {
        assert_eq!(quantize_lp(f32::NEG_INFINITY), 255);
        assert_eq!(dequantize_lp(255), f32::NEG_INFINITY);
        assert_eq!(quantize_lp(0.0), 0);
        assert_eq!(dequantize_lp(0), 0.0);
        let lp = -1.5;
        let q = quantize_lp(lp);
        assert_eq!(q, 12);
        assert_eq!(dequantize_lp(q), -1.5);
    }

    #[test]
    fn fuzz_dataview_extensive() {
        let good = sample_data();

        // 1. All truncations
        for n in 0..=good.len() {
            let _ = DataView::parse(&good[..n]);
        }

        // 2. Simple LCG PRNG for reproducible fuzzing
        let mut seed: u64 = 0x1234_5678_9ABC_DEF0;
        let mut next_rand = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed
        };

        // 3. Random bit flips across entire file
        for _ in 0..10_000 {
            let mut b = good.clone();
            let flips = (next_rand() % 8) as usize + 1;
            for _ in 0..flips {
                let pos = (next_rand() as usize) % b.len();
                let bit = (next_rand() % 8) as u8;
                b[pos] ^= 1 << bit;
            }
            if let Ok(v) = DataView::parse(&b) {
                let _ = v.header();
                let _ = v.alphabet();
                let _ = v.trie_nodes();
                let _ = v.words();
                let _ = v.rules();
                let _ = v.chunks();
                let _ = v.chlm();
                let _ = v.phrases();
                let _ = v.regions();
                let _ = v.string(0);
                let _ = v.string(100);
                let _ = v.parm();
                let _ = v.meta();
                if let Ok(Some(bi)) = v.bigrams() {
                    let _ = bi.successors_of(0);
                    let _ = bi.successors_of(100);
                }
                if let Ok(Some(di)) = v.diacritics() {
                    let _ = di.variants_for(0);
                    let _ = di.variants_for(100);
                }
            }
        }

        // 4. Pure random byte buffers
        for _ in 0..1_000 {
            let len = (next_rand() % 1024) as usize;
            let mut b = vec![0u8; len];
            for byte in &mut b {
                *byte = (next_rand() & 0xFF) as u8;
            }
            let _ = DataView::parse(&b);
        }
    }
}
