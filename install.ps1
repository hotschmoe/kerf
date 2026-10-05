# Kerf installer for Windows:  irm https://raw.githubusercontent.com/hotschmoe/kerf/main/install.ps1 | iex
# Installs kerf.exe to %LOCALAPPDATA%\kerf\bin and adds it to your user PATH.
# Pin a version:  $env:KERF_VERSION = "v0.1.0-alpha.1"; irm ... | iex
$ErrorActionPreference = 'Stop'
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
$asset = "kerf-$arch-windows.exe"
$base = if ($env:KERF_VERSION) { "https://github.com/hotschmoe/kerf/releases/download/$($env:KERF_VERSION)" } else { 'https://github.com/hotschmoe/kerf/releases/latest/download' }
$dir = Join-Path $env:LOCALAPPDATA 'kerf\bin'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$dest = Join-Path $dir 'kerf.exe'
Write-Host "downloading $asset ..."
Invoke-WebRequest -Uri "$base/$asset" -OutFile $dest -UseBasicParsing
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath) { $userPath = '' }
if (($userPath -split ';') -notcontains $dir) {
  [Environment]::SetEnvironmentVariable('Path', ($userPath.TrimEnd(';') + ";$dir").TrimStart(';'), 'User')
  Write-Host "added $dir to your user PATH (open a new terminal to pick it up)"
}
$env:Path = "$env:Path;$dir"
& $dest version
Write-Host ""
Write-Host "kerf installed: $dest"
Write-Host "next:  mkdir details; cd details; kerf init; kerf serve --open"
Write-Host "       then ask Claude Code / Grok (in that folder, or from the web console) for a detail."
