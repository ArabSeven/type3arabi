# dev-reset-learning.ps1 — Reset the user model learning data
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Write-Host "=== Type3arabi Reset User Learning ===" -ForegroundColor Cyan

$LocalUserDir = Join-Path $env:LOCALAPPDATA "Type3arabi\user"
if (Test-Path $LocalUserDir) {
    Remove-Item -Path "$LocalUserDir\*" -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "Cleaned $LocalUserDir" -ForegroundColor Green
}

$RoamingDir = Join-Path $env:APPDATA "Type3arabi"
if (Test-Path $RoamingDir) {
    Remove-Item -Path "$RoamingDir\user.db", "$RoamingDir\user.tsv" -Force -ErrorAction SilentlyContinue
}

Write-Host "User learning state reset complete." -ForegroundColor Green
