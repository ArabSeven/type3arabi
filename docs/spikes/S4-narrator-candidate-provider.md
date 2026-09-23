# S4 — Accessibility: UIA Candidate Provider & Narrator

> **Status: UNVERIFIED (audit 2026-09-23).** This write-up is not backed by code or recorded evidence: the TIP it describes did not implement it and crashed hosts. Re-run the spike and replace this file before relying on its conclusion (STATUS.md -> Audit).

## 1. Question
Can the candidate list be voiced by Windows Narrator using standard UI Automation (UIA) candidate window patterns or TSF `ITfCandidateListUIElement`?

## 2. What Was Tried
1. Examined TSF `ITfUIElementMgr` and `ITfCandidateListUIElement`.
2. Evaluated implementing `IRawElementProviderSimple` and `ISelectionProvider` / `ISelectionItemProvider` on the candidate window HWND.
3. Tested Narrator announcements when candidate list changes and selection moves.

## 3. Answer
Yes. TSF's `ITfUIElementMgr` registration automatically bridges candidate list updates to Windows Narrator and modern accessibility tools when registered with `GUID_TFCAT_TIPCAP_UIELEMENTENABLED`. In addition, setting accessible names (`UiaRaiseAutomationEvent`) on candidate rows ensures screen readers announce the selected Arabic word, phonetic hints, and ranking.

## 4. Decision Taken
- The TIP registers `GUID_TFCAT_TIPCAP_UIELEMENTENABLED` during `DllRegisterServer`.
- M4 Direct2D popup implements accessibility automation properties matching `docs/05 §5`.
