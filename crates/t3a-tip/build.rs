//! Windows resources for the TIP DLL (docs/07 §1, §3): the input-indicator icons — `101` brand (the TSF
//! profile's icon, `IDI_BRAND`, docs/02 §2), `102` / `103` mode Arabic / Latin (the tray button, docs/02 §10) —
//! in Microsoft's black-and-white IME icon style (`res/make_icons.py`), and VERSIONINFO from the crate version.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let rc = version_rc(
        "Type3arabi Arabic keyboard (TSF text input processor)",
        "t3a_tip.dll",
        0x2, // VFT_DLL
    );
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("t3a_tip.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile_for_everything(&out, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}

/// Absolute path of `res/<name>` as rc.exe wants it: no `\\?\` prefix, backslashes escaped.
fn res(name: &str) -> String {
    std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("res")
        .join(name)
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .replace(r"\\?\", "")
        .replace('\\', r"\\")
}

/// Icons + VERSIONINFO (t3a-hotkey/build.rs has the same VERSIONINFO with the color app icon).
fn version_rc(description: &str, file: &str, file_type: u32) -> String {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let nums: Vec<u16> = version
        .split(['.', '-'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let (a, b, c) = (nums[0], nums[1], nums[2]);
    let (brand, mode_ar, mode_latin) =
        (res("brand.ico"), res("mode-ar.ico"), res("mode-latin.ico"));
    format!(
        r#"#pragma code_page(65001)
101 ICON "{brand}"
102 ICON "{mode_ar}"
103 ICON "{mode_latin}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
FILEOS 0x40004
FILETYPE {file_type:#x}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Type3arabi"
      VALUE "FileDescription", "{description}"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "{file}"
      VALUE "LegalCopyright", "© 2026 Hassan Obaida. Apache-2.0"
      VALUE "OriginalFilename", "{file}"
      VALUE "ProductName", "Type3arabi"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}
