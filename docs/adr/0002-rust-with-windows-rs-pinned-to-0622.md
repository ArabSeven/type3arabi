# ADR-0002: Rust with windows-rs pinned to =0.62.2

- **Status:** Accepted (2026-09-22)
- **Deciders:** Architect, Owner

## Context
The TIP is loaded into every app. A managed runtime (.NET) inside host processes causes version conflicts and memory overhead (Maren required .NET 2.0). C++ is the traditional choice but memory-unsafe. Working Rust TIPs exist (ime-rs, ainuKey). windows-rs 0.62.2 is the latest umbrella crate; 0.100 is being prepared with metadata changes.

## Decision
All shipped native code is Rust (stable, pinned toolchain). Windows bindings via the `windows` crate pinned to `=0.62.2`; COM via `#[implement]`.

## Consequences
+ Memory safety in host processes, no runtime, small DLLs, one language for engine + TIP + tools CLI.
+ Engine is portable and testable on Linux CI.
− Fewer TSF examples in Rust than C++; mitigated by SampleIME/ime-rs as references.
− Upgrading windows-rs requires an ADR and a full compat run.

## Alternatives considered
C++ (SampleIME-based); C# (.NET in-proc, rejected); Zig (immature Windows ecosystem).
