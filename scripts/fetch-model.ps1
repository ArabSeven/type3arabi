# fetch-model.ps1 - get the release model pinned in data/model.lock.toml and verify its SHA-256
# (docs/07 §2). Used by the release workflow; locally it can verify a model you built yourself.
#
#   powershell -ExecutionPolicy Bypass -File .\scripts\fetch-model.ps1                   # download
#   powershell -ExecutionPolicy Bypass -File .\scripts\fetch-model.ps1 -From target\type3arabi-rc2.dat
# Output: target\type3arabi-release.dat (exit code 1 if the file does not match the lock).
# A private repository needs a token: $env:GH_TOKEN (the workflow passes github.token).
[CmdletBinding()]
param([string]$From, [string]$Out = "target\type3arabi-release.dat")

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$lock = Get-Content (Join-Path $RepoRoot "data\model.lock.toml") -Raw
function Field($name) {
    $m = [regex]::Match($lock, "(?m)^$name\s*=\s*""?([^""\r\n]+)""?")
    if (-not $m.Success) { throw "data/model.lock.toml: missing '$name'" }
    $m.Groups[1].Value.Trim()
}
$sha = (Field "sha256").ToLowerInvariant()
$bytes = [int64](Field "bytes")
$url = Field "url"
$dest = if ([System.IO.Path]::IsPathRooted($Out)) { $Out } else { Join-Path $RepoRoot $Out }
New-Item -ItemType Directory -Force -Path (Split-Path $dest) | Out-Null

if ($From) {
    Copy-Item $From $dest -Force
} else {
    Write-Host "Downloading $url"
    $headers = @{}
    if ($env:GH_TOKEN) { $headers["Authorization"] = "Bearer $($env:GH_TOKEN)" }
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri $url -OutFile $dest -Headers $headers -UseBasicParsing
}

$len = (Get-Item $dest).Length
$got = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLowerInvariant()
if ($len -ne $bytes -or $got -ne $sha) {
    Remove-Item $dest -Force
    Write-Error "Model does not match data/model.lock.toml (got $len bytes, sha256 $got; expected $bytes, $sha)"
    exit 1
}
Write-Host "Model OK: $dest ($len bytes, sha256 $got)"
