# 12 — `type3arabi.dat` binary format (v1)

Single file, little-endian, every section 8-byte aligned, read by zero-copy casts (`bytemuck`) over a
read-only memory map. One writer (`t3a_data::writer`), one reader (`t3a_data::DataView`). Any structural
change bumps `format_version`; readers reject unknown major versions (TIP → safe passthrough).

## 1. Header (64 bytes, offset 0)
| Off | Type | Field |
|---|---|---|
| 0 | [u8;4] | magic `b"T3AD"` |
| 4 | u16 | format_version_major (1) |
| 6 | u16 | format_version_minor (0) |
| 8 | u32 | section_count |
| 12 | u32 | flags (bit0 = has DIAC, bit1 = has BIGR) |
| 16 | u64 | data_version (YYYYMMDDnn as integer) |
| 24 | [u8;16] | build_id (random, for cache keys) |
| 40 | u64 | section_table_offset |
| 48 | u64 | file_len (must equal actual length) |
| 56 | u32 | header_crc32 (of bytes 0..56) |
| 60 | u32 | reserved |

## 2. Section table (`section_count` × 24 bytes)
`kind: [u8;4]`, `reserved: u32`, `offset: u64`, `len: u64`. Kinds (all required in v1 unless noted):
`ALPH` alphabet · `TRIE` lexicon trie nodes · `WREC` word records · `STRS` UTF-8 string pool ·
`RULE` transliteration rules · `CHNK` chunk index · `BIGR` bigrams (optional) · `CHLM` char LM ·
`PHRS` phrases · `DIAC` vocalizations (optional) · `REGN` region priors · `PARM` engine params · `META` JSON (sources, licenses, stats).

Validation on load (cheap, O(sections)): magic, versions, header CRC, file_len, every section within bounds
and aligned, required sections present, each section's own small header sane. Full-content CRCs are verified
only by `t3a-cli verify` and the installer's post-install check — never in the TIP.

## 3. `ALPH` — the T3A alphabet (fixed in v1; stored for self-description)
Code 0 is reserved (trie root / none). Codes 1–42 in Unicode order:

| Code | Char | U+ | Code | Char | U+ | Code | Char | U+ |
|---|---|---|---|---|---|---|---|---|
| 1 | ء | 0621 | 15 | د | 062F | 29 | ك | 0643 |
| 2 | آ | 0622 | 16 | ذ | 0630 | 30 | ل | 0644 |
| 3 | أ | 0623 | 17 | ر | 0631 | 31 | م | 0645 |
| 4 | ؤ | 0624 | 18 | ز | 0632 | 32 | ن | 0646 |
| 5 | إ | 0625 | 19 | س | 0633 | 33 | ه | 0647 |
| 6 | ئ | 0626 | 20 | ش | 0634 | 34 | و | 0648 |
| 7 | ا | 0627 | 21 | ص | 0635 | 35 | ى | 0649 |
| 8 | ب | 0628 | 22 | ض | 0636 | 36 | ي | 064A |
| 9 | ة | 0629 | 23 | ط | 0637 | 37 | پ | 067E |
| 10 | ت | 062A | 24 | ظ | 0638 | 38 | چ | 0686 |
| 11 | ث | 062B | 25 | ع | 0639 | 39 | ڤ | 06A4 |
| 12 | ج | 062C | 26 | غ | 063A | 40 | ڨ | 06A8 |
| 13 | ح | 062D | 27 | ف | 0641 | 41 | ڭ | 06AD |
| 14 | خ | 062E | 28 | ق | 0642 | 42 | گ | 06AF |

Section content: `count: u32` then `count × u32` code points. The engine uses the compiled-in table
(`t3a_engine::alphabet`) and asserts it equals `ALPH` at load.

## 4. `TRIE` + `WREC` + `STRS`
`TRIE`: `node_count: u32, pad: u32`, then `node_count × Node` in BFS order; node 0 = root.
```rust
#[repr(C)] struct Node {        // 12 bytes
    first_child: u32,           // index of first child (children contiguous, sorted by label); 0 if none
    word: u32,                  // WREC index + 1; 0 = not terminal
    label: u8,                  // T3A code of the edge into this node (0 for root)
    child_count: u8,            // ≤ 42
    max_q: u8,                  // best quantized mixture lp in subtree (for heuristic h)
    flags: u8,                  // bit0 = subtree has completable words (NO_COMPLETE-filtered)
}
```
`WREC`: `count: u32, pad: u32`, then records:
```rust
#[repr(C)] struct WordRec {     // 16 bytes
    q: [u8; 6],                 // per-dialect quantized lp (MSA, LEV, EGY, GLF, IRQ, MAG); 255 = unseen
    flags: u16,                 // bit0 TANWEEN_FATH, bit1 NO_COMPLETE, bit2 SACRED
    surface: u32,               // STRS offset of UTF-8 base form (len-prefixed u8)
    diac: u32,                  // DIAC entry index + 1, 0 = none
}
```
`STRS`: raw bytes; each string = `len: u8` + UTF-8 bytes.

## 5. `RULE` + `CHNK`
`RULE`: `count: u32, pad`, then:
```rust
#[repr(C)] struct Rule {        // 16 bytes
    arabic: [u8; 3],            // T3A codes, 0-padded; arabic_len says how many
    arabic_len: u8,             // 0..=3
    pos_mask: u8,               // bit0 I, bit1 M, bit2 F
    flags: u8,                  // GEM, TANWEEN, VOWEL(kind in bits 4-6), ARTICLE
    q_any: u8,                  // lp for dialect '*'
    _pad: u8,
    q: [u8; 6],                 // per-dialect lp, 255 = use q_any
    chunk: u16,                 // CHNK index
}
```
`CHNK`: `count: u32, pad`, then `count × { latin: [u8; 4] (0-padded ASCII/internal symbols), len: u8, pad: u8, first_rule: u16, rule_count: u16, pad: u16 }` sorted by latin bytes.
Internal symbols for emphatics: `0x01`=Ŧ(T), `0x02`=Ş(S), `0x03`=Đ(D), `0x04`=Ẓ(Z), `0x05`=Ḥ(H).

## 6. `BIGR`
`prev_count: u32, pair_count: u32`, then `offsets: [u32; lexicon_count + 1]` (CSR: successors of word `w`
are `pairs[offsets[w]..offsets[w+1]]`), then `pair_count × { next: u32, q: u8, pad: [u8;3] }` sorted by `next`.

## 7. `CHLM`
`capacity: u32 (power of 2), pad`, then `capacity × { fp: u32, q: u8, order: u8, pad: u16 }`, open addressing
with linear probing; key = FNV-1a-32 of `(order, context codes…, next code)`; `fp = 0` empty slot.
Backoff α = 0.4 (ln α added per backoff step), order 5 → 1.

## 8. `PHRS`
Sorted array of `{ key: STRS offset, out: STRS offset, dialect_mask: u8, flags: u8 (bit0 default), pad: u16 }`;
binary search by key bytes.

## 9. `DIAC`
`count: u32, pad`, then `count × { first: u32, n: u8, pad: [u8;3] }` and a variant array `{ s: STRS offset, q: u8, dialect_mask: u8, pad: u16 }`.

## 10. `REGN`, `PARM`, `META`
- `REGN`: rows `{ iso2: [u8;2], pad: [u8;2], weights: [f32; 6] }`.
- `PARM`: UTF-8 TOML text (tiny; parsed once at load by the engine's minimal TOML reader).
- `META`: UTF-8 JSON (not read by the TIP; for `t3a-cli inspect`/About page): sources + licenses + counts.
