# Builds KYNEX's installers into target\dist: KYNEX-<version>-setup.exe (Inno
# Setup, for this user or all), KYNEX-<version>.msi (WiX v5, for IT) and
# KYNEX-<version>-windows-x64.zip (the programs alone). The version is the
# build's (tools/version.rs): commit first.
#
#   powershell -ExecutionPolicy Bypass -File tools\package.ps1
#
# Needs: Rust (MSVC), Inno Setup 6 (winget install JRSoftware.InnoSetup) and
# WiX 5 (dotnet tool install --global wix --version 5.0.2). WiX 6 and later
# carry a maintenance-fee licence; 5 does not.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

cargo build --release -p kynex
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$bin = Join-Path $root 'target\release'
# The version the programs were built as (tools/version.rs): x.y.<build>.
$version = ((& (Join-Path $bin 'kynex.exe') --version) -split ' ')[1]
if ($version -like '*-dev') { throw "KYNEX ${version}: commit first, a release is built from a commit" }
Write-Host "KYNEX $version"
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

# And the programs on their own, to run without installing.
$zip = Join-Path $out "KYNEX-$version-windows-x64.zip"
Compress-Archive -Force -Path (Join-Path $bin 'kynex.exe'), (Join-Path $bin 'kynex-studio.exe') -DestinationPath $zip

Get-ChildItem $out -File | Where-Object { $_.Extension -in '.exe', '.msi', '.zip' } |
    Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } }
