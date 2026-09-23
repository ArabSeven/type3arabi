//! Fixed identities (docs/02 §1). NEVER change after the first public release.

/// TIP CLSID {8A4B9277-1E2E-45E0-92A2-83FED833D8BF}
pub const CLSID_TIP: u128 = 0x8A4B9277_1E2E_45E0_92A2_83FED833D8BF;
/// Profile GUID {90D49398-54D3-4F08-9C15-0B38D0820A87}
pub const GUID_PROFILE: u128 = 0x90D49398_54D3_4F08_9C15_0B38D0820A87;
/// Display attribute: composing preview {3F94CF8A-3689-49C9-9597-18F7435E0FE8}
pub const GUID_DISPLAY_ATTR_INPUT: u128 = 0x3F94CF8A_3689_49C9_9597_18F7435E0FE8;
/// Display attribute: tashkeel editing {5AD3E153-E0D0-41BA-8D1E-E8B312230016}
pub const GUID_DISPLAY_ATTR_TASHKEEL: u128 = 0x5AD3E153_E0D0_41BA_8D1E_E8B312230016;
/// Private compartment: engine mode {62E8BEAD-7142-47C3-9555-656FC8A9E8FA}
pub const GUID_COMPARTMENT_MODE: u128 = 0x62E8BEAD_7142_47C3_9555_656FC8A9E8FA;
/// Preserved key: Arabic/Latin toggle {35746905-7855-46BE-A409-AF7CC35C8FF7}
pub const GUID_PRESERVED_TOGGLE: u128 = 0x35746905_7855_46BE_A409_AF7CC35C8FF7;
/// MSI UpgradeCode {C71F4AAE-43FF-47DA-B6F0-582B0611BAF6} (used by installer/, kept here for one source of truth)
pub const MSI_UPGRADE_CODE: u128 = 0xC71F4AAE_43FF_47DA_B6F0_582B0611BAF6;
/// Reserved (reconversion function provider) {51D93195-3C9E-4C04-BDE1-419D15F1015C}
pub const GUID_RESERVED_RECONVERSION: u128 = 0x51D93195_3C9E_4C04_BDE1_419D15F1015C;

/// Profile description shown by Windows.
pub const PROFILE_DESCRIPTION: &str = "Type3arabi";

/// The single LANGID the profile is registered under (docs/02 §2.1, Owner decision O10):
/// ar-SA. Users see exactly one entry, "Arabic (Saudi Arabia) · Type3arabi"; dialects are an engine
/// concern, never a user-visible keyboard choice.
pub const LANGID: u16 = 0x0401;

/// Every Arabic LANGID an earlier dev build registered the profile under. Registration no longer
/// uses these; `DllUnregisterServer` removes the profile from all of them so old installs are cleaned.
pub const LEGACY_LANGIDS: [u16; 16] = [
    0x0401, 0x0801, 0x0C01, 0x1001, 0x1401, 0x1801, 0x1C01, 0x2001, 0x2401, 0x2801, 0x2C01, 0x3001,
    0x3401, 0x3801, 0x3C01, 0x4001,
];

/// `dwExtraInfo` tag for keys we re-inject (docs/02 §5.3): "T3AR".
pub const REINJECT_MAGIC: usize = 0x5433_4152;

/// Format a GUID as registry text `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
pub fn guid_string(g: u128) -> String {
    let h = format!("{g:032X}");
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guid_text_matches_docs() {
        assert_eq!(
            guid_string(CLSID_TIP),
            "{8A4B9277-1E2E-45E0-92A2-83FED833D8BF}"
        );
        assert_eq!(
            guid_string(GUID_PROFILE),
            "{90D49398-54D3-4F08-9C15-0B38D0820A87}"
        );
        let docs = include_str!("../../../docs/02-tsf-integration.md");
        for g in [
            CLSID_TIP,
            GUID_PROFILE,
            GUID_DISPLAY_ATTR_INPUT,
            GUID_DISPLAY_ATTR_TASHKEEL,
            GUID_COMPARTMENT_MODE,
            GUID_PRESERVED_TOGGLE,
            MSI_UPGRADE_CODE,
            GUID_RESERVED_RECONVERSION,
        ] {
            assert!(
                docs.contains(&guid_string(g)),
                "{} missing from docs/02 §1",
                guid_string(g)
            );
        }
        assert_eq!(LANGID & 0x3FF, 0x01); // LANG_ARABIC
        assert!(LEGACY_LANGIDS.iter().all(|l| l & 0x3FF == 0x01));
        assert!(LEGACY_LANGIDS.contains(&LANGID));
    }
}
