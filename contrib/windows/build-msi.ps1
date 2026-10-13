<#
.SYNOPSIS
Builds UpNext's per-user Windows installer from upnext.exe and upnext-app.exe.

.DESCRIPTION
Runs WiX Toolset 3.14.1 on contrib\windows\upnext.wxs and writes
upnext-x86_64-pc-windows-msvc.msi and its .sha256 to -OutDir. The release
workflow runs it on a v* tag with the programs from dist's Windows build, and
a development build runs it on a Windows machine after
`cargo build --release --bins`; contrib\windows\README.md has both.

Without -WixDir it downloads WiX's binaries zip from WiX's GitHub release
into target\wix-3.14.1 the first time, checks it against the SHA-256 below,
and unpacks it there; nothing is installed. WiX 3 runs on the .NET Framework
4.8 that Windows 10 and 11 include.

Relative paths are relative to the checkout (the folder two above this one).
Works in Windows PowerShell 5.1 and PowerShell 7.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File contrib\windows\build-msi.ps1
#>
[CmdletBinding()]
param(
    # The folder with upnext.exe and upnext-app.exe.
    [string]$BinDir = 'target\release',
    # Where the MSI and its .sha256 are written.
    [string]$OutDir = 'target\wix',
    # A folder with WiX 3.14's candle.exe and light.exe, instead of the
    # downloaded copy.
    [string]$WixDir = '',
    # The installer's version, major.minor.patch. By default Cargo.toml's
    # version without any pre-release part (Windows Installer takes numbers
    # only). A higher one is a way to test upgrading.
    [string]$Version = ''
)

Set-StrictMode -Version 3
$ErrorActionPreference = 'Stop'
# Windows PowerShell 5.1 downloads many times slower with its progress bar.
$ProgressPreference = 'SilentlyContinue'

# WiX 3.14.1 (wix3141rtm), the last WiX 3 release, the version GitHub's
# Windows runners also carry. A new version needs this hash too: the SHA-256
# of the new zip.
$WixUrl = 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip'
$WixSha256 = '6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31'
$MsiName = 'upnext-x86_64-pc-windows-msvc.msi'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Source = Join-Path $Root 'contrib\windows'

function Get-RootPath([string]$Path) {
    if ([IO.Path]::IsPathRooted($Path)) { return $Path }
    return Join-Path $Root $Path
}

# A native program's failure doesn't stop PowerShell by itself.
function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$(Split-Path -Leaf $Program) failed with exit code $LASTEXITCODE"
    }
}

$BinDir = Get-RootPath $BinDir
$OutDir = Get-RootPath $OutDir
foreach ($exe in 'upnext.exe', 'upnext-app.exe') {
    if (-not (Test-Path -LiteralPath (Join-Path $BinDir $exe) -PathType Leaf)) {
        throw "$exe isn't in $BinDir. Build both programs first: cd web; npm ci; npm run build; cd ..; cargo build --release --bins"
    }
}

# The licences and the third-party notices go in beside the programs.
foreach ($file in 'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD_PARTY_NOTICES.txt') {
    if (-not (Test-Path -LiteralPath (Join-Path $Root $file) -PathType Leaf)) {
        throw "$file isn't in $Root; THIRD_PARTY_NOTICES.txt comes from: node contrib\notices\generate.mjs"
    }
}

if (-not $Version) {
    $line = Select-String -LiteralPath (Join-Path $Root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $line) { throw "Cargo.toml has no version line" }
    $Version = $line.Matches[0].Groups[1].Value -replace '[-+].*$', ''
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "The installer's version must be major.minor.patch in numbers, not '$Version'"
}

if ($WixDir) {
    $WixDir = Get-RootPath $WixDir
} else {
    $WixDir = Join-Path $Root 'target\wix-3.14.1'
    if (-not (Test-Path -LiteralPath (Join-Path $WixDir 'light.exe'))) {
        New-Item -ItemType Directory -Force -Path $WixDir | Out-Null
        $zip = Join-Path $WixDir 'wix314-binaries.zip'
        Write-Host "Downloading WiX 3.14.1 from $WixUrl"
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -UseBasicParsing -Uri $WixUrl -OutFile $zip
        $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash
        if ($got -ne $WixSha256) {
            Remove-Item -LiteralPath $zip
            throw "The WiX download's SHA-256 is $got, not $WixSha256; it was deleted and nothing was unpacked"
        }
        Expand-Archive -LiteralPath $zip -DestinationPath $WixDir -Force
        Remove-Item -LiteralPath $zip
    }
}
$Candle = Join-Path $WixDir 'candle.exe'
$Light = Join-Path $WixDir 'light.exe'
foreach ($tool in $Candle, $Light) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "$tool doesn't exist" }
}

$Obj = Join-Path $OutDir 'obj'
New-Item -ItemType Directory -Force -Path $Obj | Out-Null
$Msi = Join-Path $OutDir $MsiName
$WixObj = Join-Path $Obj 'upnext.wixobj'

Write-Host "Building $MsiName $Version from $BinDir"
Invoke-Checked $Candle @(
    '-nologo', '-arch', 'x64', '-ext', 'WixUIExtension', '-ext', 'WixUtilExtension',
    "-dVersion=$Version", "-dBinDir=$BinDir", "-dSourceDir=$Source", "-dRootDir=$Root",
    '-out', $WixObj, (Join-Path $Source 'upnext.wxs')
)
# ICE61 warns that builds of the same version replace each other, and ICE91
# that per-user files aren't installed for every user: both are the point.
Invoke-Checked $Light @(
    '-nologo', '-ext', 'WixUIExtension', '-ext', 'WixUtilExtension', '-cultures:en-us',
    '-loc', (Join-Path $Source 'upnext.wxl'),
    '-sice:ICE61', '-sice:ICE91', '-spdb',
    '-out', $Msi, $WixObj
)
Remove-Item -Recurse -Force -LiteralPath $Obj

# The same form as dist's checksums beside the archives: `sha256sum -c`
# reads it, and so does a person comparing it with Get-FileHash.
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Msi).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$Msi.sha256", "$hash *$MsiName`n", (New-Object Text.UTF8Encoding $false))
Write-Host "Wrote $Msi"
Write-Host "SHA-256 $hash"
