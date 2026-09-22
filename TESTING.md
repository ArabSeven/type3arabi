# Type3arabi — Local Testing & Verification Guide

This guide describes how to install, test, and verify **Type3arabi** on your Windows machine in real applications (Notepad, Chrome, Word, WhatsApp, etc.).

---

## 1. Quick Installation (One Command)

From a PowerShell prompt in the repository root (the script will automatically request Administrator elevation to register the TSF Text Input Processor):

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev-install.ps1
```

This automated script will:
1. Compile the release TIP DLLs for both **64-bit** (`x86_64`) and **32-bit** (`i686`) Windows processes.
2. Compile and package the binary data file (`type3arabi.dat`) sitting directly beside the DLLs.
3. Register the in-proc COM server using `regsvr32` for 64-bit and 32-bit (SysWOW64) subsystems.
4. Register and activate the TSF Text Input Processor profile **AR – Type3arabi** (Arabic / Saudi Arabia `0401`).
5. Configure proper AppContainer ACL permissions on `%LOCALAPPDATA%\Type3arabi` for UWP/Packaged apps.

---

## 2. Using Type3arabi in Any Windows App

### Step 1: Switch Input Method
Press **Win + Space** (or **Alt + Shift**) until the language indicator in your taskbar shows:
> **ع** · **AR – Type3arabi**

### Step 2: Open Any Application
Open **Notepad**, **Microsoft Word**, **Google Chrome**, **WhatsApp Desktop**, or the Windows Search box.

### Step 3: Type Arabizi & Verify Key Features

| What to Type | Key Actions | Expected Behavior |
|---|---|---|
| `mar7aba` | Press **Space** | Candidate popup appears showing `مرحبا` highlighted. Space commits `مرحبا ` with a trailing space. |
| `3allam` | Press **Ctrl + Enter** | "Harakat from your vowels" commits `عَلَّم` with fatha and shadda directly into the document. |
| `allah` | Press **Space** | Candidate popup offers `اللّه` (with shadda) as rank 1 and `الله` as rank 2. Space commits `اللّه `. |
| `shukran` | Press **Space** | Adverbial tanween styling commits `شكراً` automatically. |
| `3ilm` | Press **Tab** | Opens the **In-Popup Tashkeel Editor**. Displays quick picks (e.g. `① عِلم ✦من كتابتك`, `② عِلْم`, `③ عَلَم`) and 10-item mark palette. |
| Inside Tashkeel Editor | Press **1**–**8** or **Enter** | **1**–**8** loads quick pick; **Enter** inserts the vocalized word without space; **Space** inserts with space; **Esc** reverts to candidate list. |
| Inside Tashkeel Editor | Press **a, u, i, o, w, A, U, I, ^, x** | Applies marks (fatha, damma, kasra, sukun, shadda, tanween, dagger alif, clear). Visual **← / →** navigates between letters in RTL. |
| `kull` + Space | Press **Backspace** | Undo re-edit anchor: first Backspace deletes trailing space; second Backspace restores the Latin composition buffer `kull` and popup with previous choice highlighted. |
| `,` `;` `?` | Type punctuation | Automatically maps to Arabic punctuation `،` `؛` `؟`. |
| `Ctrl + Space` | Hotkey | Seamlessly toggles between Arabic Arabizi mode and raw Latin passthrough typing. |

---

## 3. Maintenance & Developer Scripts

### Clean Uninstallation
To remove the keyboard layout and unregister all DLLs from Windows:
```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev-uninstall.ps1
```

### Reset Learned User Model
To clear all locally learned user choices and custom words:
```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev-reset-learning.ps1
```

### Full Workspace Build
To build all portable and Windows crates:
```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\dev-build.ps1 -Release
```

---

## 4. Autonomous Verification & Accuracy Commands

You can also run all validation suites directly from the command line:

```bash
# Run all workspace unit tests and invariants (42 engine tests, 7 TIP tests, 5 data tests):
cargo test --workspace

# Run per-keystroke latency benchmark (P1 budget: p50 <= 0.8ms, p99 <= 3.0ms):
cargo run -p t3a-cli --release -- bench

# Run full dialect accuracy evaluation suite against Gate E2 targets:
cargo run -p t3a-cli --release -- eval data/eval/smoke.tsv

# Inspect engine alignment, features, and vowel-derived harakat for any Arabizi word:
cargo run -p t3a-cli -- explain 3allam --dialect LEV
cargo run -p t3a-cli -- explain shukran
cargo run -p t3a-cli -- explain allah

# Interactive CLI typing simulator (terminal REPL):
cargo run -p t3a-cli -- repl --dialect LEV
```
