<#
.SYNOPSIS
  Build, sign and package Rewind OS for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    rewind-os-<version>-windows-<arch>-portable.zip   rewind-os.exe, rewind-os-cli.exe, README and licences
    rewind-os-<version>-windows-<arch>-portable.zip.sha256
    rewind-os-<version>-windows-<arch>.msi              installer (Program Files, Start menu)
    rewind-os-<version>-windows-<arch>.msi.sha256

  The installer is built with WiX 5 (packaging/windows/rewind-os.wxs), installed on the fly as a
  .NET tool when `wix` is not on the PATH. MSI versions are numbers only, so a pre-release
  (0.6.0-rc.1) gets no installer; the portable zip is still built.

  The binaries link the C runtime statically (+crt-static), so no Visual C++ redistributable is
  needed. They are signed when WINDOWS_CERTIFICATE (base64 .pfx) and WINDOWS_CERTIFICATE_PASSWORD
  are set; otherwise they are left unsigned and the script says so.

  Needs: Rust with the MSVC toolchain and the target, and the Windows SDK (rc.exe, signtool.exe).

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch arm64 -SkipBuild
#>
param(
  [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

function Write-Summary([string] $Text) {
  Write-Output $Text
  if ($env:GITHUB_STEP_SUMMARY) { Add-Content -Path $env:GITHUB_STEP_SUMMARY -Value $Text }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:REWIND_OS_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } 'arm64' { 'aarch64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$Bins = @('rewind-os', 'rewind-os-cli')

Write-Output "Rewind OS $Version for Windows $Arch ($Target)"

if (-not $SkipBuild) {
  # Static C runtime, scoped to the target so build scripts and proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  $pkgs = @(); foreach ($b in $Bins) { $pkgs += @('-p', $b) }
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked @pkgs --target $Target }
}
$Bin = Join-Path $TargetDir "$Target\release"

# Machine (COFF header) must match -Arch, so an x64 build never ships labelled arm64.
# Subsystem: 2 = Windows GUI (no console window behind the app), 3 = console (the CLI prints).
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
$Machine = switch ($Arch) { 'x64' { 0x8664 } 'arm64' { 0xAA64 } }
foreach ($check in @(@('rewind-os.exe', 2), @('rewind-os-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}

$Name = "rewind-os-$Version-windows-$Arch-portable"
$Stage = Join-Path $TargetDir "windows-package\$Name"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
$Exes = $Bins | ForEach-Object { Join-Path $Stage "$_.exe" }
foreach ($b in $Bins) { Copy-Item (Join-Path $Bin "$b.exe") $Stage }

# ---- signing -----------------------------------------------------------------------------------
if ($env:WINDOWS_CERTIFICATE -and $env:WINDOWS_CERTIFICATE_PASSWORD) {
  $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $signtool) { throw 'signtool.exe not found in the Windows SDK' }
  $pfx = Join-Path ([System.IO.Path]::GetTempPath()) "rewind-os-$([guid]::NewGuid()).pfx"
  try {
    [System.IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    Invoke-Native 'signtool sign' {
      & $signtool.FullName sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com `
        /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD @Exes
    }
  } finally {
    Remove-Item -Force $pfx -ErrorAction SilentlyContinue
  }
  Write-Summary "Windows $Arch`: signed."
} else {
  Write-Warning 'No WINDOWS_CERTIFICATE: the executables are not signed.'
  Write-Summary "Windows $Arch`: **unsigned** (no WINDOWS_CERTIFICATE secret). SmartScreen will warn on first launch."
}

# ---- portable zip ------------------------------------------------------------------------------
foreach ($f in 'README.md', 'README.es.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') {
  Copy-Item (Join-Path $Root $f) $Stage
}
$Zip = Join-Path $Dist "$Name.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Stage -DestinationPath $Zip
$Hash = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
Set-Content -Path "$Zip.sha256" -Value "$Hash  $Name.zip" -NoNewline -Encoding ascii

# ---- installer ---------------------------------------------------------------------------------
if ($Version -match '^\d+\.\d+\.\d+$') {
  $wix = Get-Command wix -ErrorAction SilentlyContinue
  if (-not $wix) {
    # Outside target\: the build cache restores target\ only partly, which leaves a broken tool.
    $toolDir = Join-Path ([System.IO.Path]::GetTempPath()) 'rewind-os-wix-5.0.2'
    $toolExe = Join-Path $toolDir 'wix.exe'
    if (-not (Test-Path $toolExe)) {
      Remove-Item -Recurse -Force $toolDir -ErrorAction SilentlyContinue
      Invoke-Native 'install WiX 5' { dotnet tool install wix --version 5.0.2 --tool-path $toolDir }
    }
    $wix = Get-Command $toolExe
  }
  $Msi = Join-Path $Dist "rewind-os-$Version-windows-$Arch.msi"
  Remove-Item -Force $Msi -ErrorAction SilentlyContinue
  Invoke-Native 'wix build' {
    & $wix.Source build (Join-Path $PSScriptRoot 'rewind-os.wxs') -arch $Arch -o $Msi `
      -d "Version=$Version" -d "BinDir=$Stage" -d "DocDir=$Stage" `
      -d "IconFile=$(Join-Path $Root 'assets\app-icon\rewind-os.ico')"
  }
  if ($env:WINDOWS_CERTIFICATE -and $env:WINDOWS_CERTIFICATE_PASSWORD) {
    $pfx = Join-Path ([System.IO.Path]::GetTempPath()) "rewind-os-$([guid]::NewGuid()).pfx"
    try {
      [System.IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
      Invoke-Native 'signtool sign (msi)' {
        & $signtool.FullName sign /fd SHA256 /td SHA256 /tr http://timestamp.digicert.com `
          /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD $Msi
      }
    } finally {
      Remove-Item -Force $pfx -ErrorAction SilentlyContinue
    }
  }
  # WiX also writes debug symbols (.wixpdb) next to the package; they are not shipped.
  Remove-Item -Force ([System.IO.Path]::ChangeExtension($Msi, '.wixpdb')) -ErrorAction SilentlyContinue
  $MsiHash = (Get-FileHash -Algorithm SHA256 $Msi).Hash.ToLowerInvariant()
  Set-Content -Path "$Msi.sha256" -Value "$MsiHash  $(Split-Path -Leaf $Msi)" -NoNewline -Encoding ascii
  Get-Item $Msi | Format-Table Name, Length
} else {
  Write-Summary "Windows $Arch`: no installer for pre-release $Version (MSI versions are numbers only)."
}

# ---- smoke test --------------------------------------------------------------------------------
$HostArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Arch -eq 'x64' -or $HostArch -eq 'arm64') {
  $cli = Join-Path $Stage 'rewind-os-cli.exe'
  Invoke-Native 'rewind-os-cli --version' { & $cli --version | Out-Host }
  Invoke-Native 'rewind-os-cli overview' { & $cli --json overview | Out-Null }
} else {
  Write-Output "skipping the smoke test: a $Arch build does not run on this $HostArch machine"
}
Get-Item $Zip, "$Zip.sha256" | Format-Table Name, Length
