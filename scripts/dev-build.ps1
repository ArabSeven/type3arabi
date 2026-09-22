# dev-build.ps1 — Build all Type3arabi workspace crates and both 64-bit and 32-bit TIP DLLs
[CmdletBinding()]
param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
Write-Host "=== Building Type3arabi ===" -ForegroundColor Cyan

$buildArgs = if ($Release) { @("--release") } else { @() }

Write-Host "1. Testing workspace..." -ForegroundColor Yellow
cargo test --workspace
if ($LASTEXITCODE -ne 0) { throw "Workspace tests failed" }

Write-Host "2. Building x86_64 TIP DLL..." -ForegroundColor Yellow
cargo build -p t3a-tip @buildArgs --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw "x86_64 build failed" }

Write-Host "3. Building i686 TIP DLL..." -ForegroundColor Yellow
cargo build -p t3a-tip @buildArgs --target i686-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw "i686 build failed" }

Write-Host "4. Building t3a-cli..." -ForegroundColor Yellow
cargo build -p t3a-cli @buildArgs
if ($LASTEXITCODE -ne 0) { throw "t3a-cli build failed" }

Write-Host "=== Build succeeded! ===" -ForegroundColor Green
