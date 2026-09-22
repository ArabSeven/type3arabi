//! Typed record definitions for binary sections (docs/12).
//!
//! All structs are `#[repr(C)]`, 8-byte or naturally aligned, and derive `bytemuck::{Pod, Zeroable}`.

use bytemuck::{Pod, Zeroable};

/// Lexicon trie node (docs/12 §4, 12 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Node {
    /// Index of first child in the TRIE section; 0 if none.
    pub first_child: u32,
    /// WREC index + 1; 0 if this node is not terminal (not a word).
    pub word: u32,
    /// T3A alphabet code of the edge leading into this node (0 for root).
    pub label: u8,
    /// Number of contiguous children.
    pub child_count: u8,
    /// Maximum quantized mixture log-prob in the subtree (for search heuristic h).
    pub max_q: u8,
    /// Subtree flags: bit0 = subtree has completable words.
    pub flags: u8,
}

/// Word record (docs/12 §4, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct WordRec {
    /// Per-dialect quantized log-prob: MSA, LEV, EGY, GLF, IRQ, MAG. 255 = unseen.
    pub q: [u8; 6],
    /// Word flags: bit0 TANWEEN_FATH, bit1 NO_COMPLETE, bit2 SACRED.
    pub flags: u16,
    /// Offset in the STRS section to the length-prefixed UTF-8 base form.
    pub surface: u32,
    /// DIAC entry index + 1; 0 = no vocalization variants.
    pub diac: u32,
}

/// Transliteration rule (docs/12 §5, 16 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Rule {
    /// T3A alphabet codes for Arabic output, 0-padded.
    pub arabic: [u8; 3],
    /// Number of active Arabic symbols in `arabic` (0..=3).
    pub arabic_len: u8,
    /// Position bitmask: bit0 Initial, bit1 Medial, bit2 Final.
    pub pos_mask: u8,
    /// Rule flags: GEM, TANWEEN, VOWEL, ARTICLE.
    pub flags: u8,
    /// Quantized log-prob for dialect `*`.
    pub q_any: u8,
    pub _pad: u8,
    /// Per-dialect quantized log-prob (255 = fall back to q_any).
    pub q: [u8; 6],
    /// Index into CHNK section.
    pub chunk: u16,
}

/// Latin chunk index entry (docs/12 §5, 12 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Chunk {
    /// Latin characters / internal emphatic symbols (0-padded ASCII).
    pub latin: [u8; 4],
    /// Number of Latin characters (1..=4).
    pub len: u8,
    pub pad: u8,
    /// Index of first rule in the RULE section.
    pub first_rule: u16,
    /// Number of rules for this chunk.
    pub rule_count: u16,
    pub pad2: u16,
}

/// Bigram pair record (docs/12 §6, 8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct BigramPair {
    /// WREC index of successor word.
    pub next: u32,
    /// Quantized bigram conditional log-prob.
    pub q: u8,
    pub pad: [u8; 3],
}

/// Character LM hash table entry (docs/12 §7, 8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct ChlmEntry {
    /// FNV-1a-32 hash fingerprint of (order, context..., next). 0 = empty slot.
    pub fp: u32,
    /// Quantized conditional log-prob.
    pub q: u8,
    /// Order of the n-gram (1..=5).
    pub order: u8,
    pub pad: u16,
}

/// Phrase dictionary entry (docs/12 §8, 12 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct PhraseEntry {
    /// STRS offset of normalized Latin key.
    pub key: u32,
    /// STRS offset of Arabic output phrase.
    pub out: u32,
    /// Bitmask of supported dialects.
    pub dialect_mask: u8,
    /// Flags: bit0 = default candidate.
    pub flags: u8,
    pub pad: u16,
}

/// Diacritic index header (docs/12 §9, 8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct DiacHeader {
    /// Index of first variant in the DIAC variants array.
    pub first: u32,
    /// Number of vocalized variants for this word.
    pub n: u8,
    pub pad: [u8; 3],
}

/// Diacritic vocalization variant (docs/12 §9, 8 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct DiacVariant {
    /// STRS offset of fully vocalized Arabic string.
    pub s: u32,
    /// Quantized conditional log-prob P(variant | base).
    pub q: u8,
    /// Bitmask of dialects in which this vocalization is valid.
    pub dialect_mask: u8,
    pub pad: u16,
}

/// Region dialect prior entry (docs/12 §10, 28 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct RegionEntry {
    /// Two-letter ISO country code (e.g. `b"SA"`).
    pub iso2: [u8; 2],
    pub pad: [u8; 2],
    /// Dialect mixture weights: MSA, LEV, EGY, GLF, IRQ, MAG.
    pub weights: [f32; 6],
}
