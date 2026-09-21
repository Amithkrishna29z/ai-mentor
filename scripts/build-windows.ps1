# Release build for Windows.
$ErrorActionPreference = "Stop"

Set-Location (Join-Path $PSScriptRoot "..")
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path (Get-Location) "target\release\ai-mentor.exe"
Write-Output ""
Write-Output "Built: $exe"
Write-Output "Run it with: .\target\release\ai-mentor.exe"
