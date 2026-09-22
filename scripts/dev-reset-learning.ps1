# dev-reset-learning.ps1 — Reset the user model learning data
[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Write-Host "=== Type3arabi Reset User Learning ===" -ForegroundColor Cyan

$UserDataDir = Join-Path $env:APPDATA "Type3arabi"
$UserDb = Join-Path $UserDataDir "user.db"
$UserTsv = Join-Path $UserDataDir "user.tsv"

if (Test-Path $UserDb) {
    Remove-Item -Path $UserDb -Force
    Write-Host "Removed $UserDb" -ForegroundColor Green
}

if (Test-Path $UserTsv) {
    Remove-Item -Path $UserTsv -Force
    Write-Host "Removed $UserTsv" -ForegroundColor Green
}

Write-Host "User learning state reset complete." -ForegroundColor Green
