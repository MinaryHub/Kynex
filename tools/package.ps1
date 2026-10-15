# Builds KYNEX's installers into target\dist: KYNEX-<version>-setup.exe (Inno
# Setup, for this user or all) and KYNEX-<version>.msi (WiX v5, for IT).
#
#   powershell -ExecutionPolicy Bypass -File tools\package.ps1
#
# Needs: Rust (MSVC), Inno Setup 6 (winget install JRSoftware.InnoSetup) and
# WiX 5 (dotnet tool install --global wix --version 5.0.2). WiX 6 and later
# carry a maintenance-fee licence; 5 does not.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
Write-Host "KYNEX $version"

cargo build --release -p kynex
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$bin = Join-Path $root 'target\release'
$out = Join-Path $root 'target\dist'
New-Item -ItemType Directory -Force $out | Out-Null

$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 not found (winget install JRSoftware.InnoSetup)' }
& $iscc /Q "/DVersion=$version" "/DBin=$bin" "/DOut=$out" installer\kynex.iss
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }

Push-Location installer
try {
    wix build kynex.wxs -arch x64 -d "Version=$version" -d "Bin=$bin" -o (Join-Path $out "KYNEX-$version.msi")
    if ($LASTEXITCODE -ne 0) { throw 'WiX failed' }
} finally { Pop-Location }

Get-ChildItem $out -File | Where-Object { $_.Extension -in '.exe', '.msi' } |
    Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
