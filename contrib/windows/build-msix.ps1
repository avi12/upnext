<#
.SYNOPSIS
Builds UpNext's Microsoft Store package (MSIX) from upnext.exe and upnext-app.exe.

.DESCRIPTION
Lays out the two programs, the licences, the third-party notices and the
pictures in contrib\windows\msix\Assets beside the manifest from
contrib\windows\msix\AppxManifest.xml, indexes the pictures with MakePri and
packs the folder with MakeAppx, both from the Windows SDK. Writes
upnext-x86_64-pc-windows-msvc.msix and its .sha256 to -OutDir, and leaves the
unpacked folder in -OutDir\layout, which a Windows machine in Developer Mode
can register to test the package without signing it
(contrib\windows\README.md).

The package isn't signed: the Microsoft Store signs what it publishes. The
identity and publisher are Partner Center's values for the reserved app
(Product management, Product identity); without them it builds a test
package with placeholders, which the Store won't take.

The release workflow runs it on a v* tag with the programs from dist's
Windows build, and the windows workflow by hand after building them;
contrib\windows\README.md has both. Works in Windows PowerShell 5.1 and
PowerShell 7.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File contrib\windows\build-msix.ps1
#>
[CmdletBinding()]
param(
    # The folder with upnext.exe and upnext-app.exe.
    [string]$BinDir = 'target\release',
    # Where the package, its .sha256 and the unpacked layout are written.
    [string]$OutDir = 'target\msix',
    # Package/Identity/Name from Partner Center.
    [string]$IdentityName = 'UpNext.Test',
    # Package/Identity/Publisher from Partner Center, CN=...
    [string]$Publisher = 'CN=UpNext Test',
    # Package/Properties/PublisherDisplayName from Partner Center.
    [string]$PublisherDisplayName = 'UpNext contributors',
    # The name reserved in Partner Center, which the Store shows.
    [string]$DisplayName = 'UpNext',
    # The package's version, major.minor.patch, to which the Store's
    # fourth number, 0, is added. By default Cargo.toml's version without
    # any pre-release part.
    [string]$Version = '',
    # A folder with the Windows SDK's makeappx.exe and makepri.exe, instead
    # of the newest one installed.
    [string]$SdkDir = ''
)

Set-StrictMode -Version 3
$ErrorActionPreference = 'Stop'

$PackageName = 'upnext-x86_64-pc-windows-msvc.msix'
# build-msix.ps1 writes this beside the programs; UpNext reads it
# (src/msix.rs) to know it runs from the package.
$Marker = 'msix-package.txt'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Source = Join-Path $Root 'contrib\windows\msix'

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
    throw "The package's version must be major.minor.patch in numbers, not '$Version'"
}
# The Store keeps the fourth number for itself and takes only 0 there.
$PackageVersion = "$Version.0"

if ($IdentityName -notmatch '^[-.A-Za-z0-9]{3,50}$') {
    throw "The identity name must be 3 to 50 letters, digits, dots and dashes, as Partner Center shows it, not '$IdentityName'"
}
if ($Publisher -notmatch '^CN=') {
    throw "The publisher must be Partner Center's CN=... value, not '$Publisher'"
}

if ($SdkDir) {
    $SdkDir = Get-RootPath $SdkDir
} else {
    # The newest Windows 10/11 SDK's x64 tools, which GitHub's Windows
    # runners carry and Visual Studio's C++ workload installs.
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem -Path (Join-Path $kits '*\x64\makeappx.exe') -ErrorAction SilentlyContinue |
        Where-Object { $_.Directory.Parent.Name -match '^\d+(\.\d+){3}$' } |
        Sort-Object { [version]$_.Directory.Parent.Name } |
        Select-Object -Last 1
    if (-not $found) { throw "No Windows SDK in $($kits): install it (Visual Studio's 'Desktop development with C++'), or pass -SdkDir" }
    $SdkDir = $found.DirectoryName
}
$MakeAppx = Join-Path $SdkDir 'makeappx.exe'
$MakePri = Join-Path $SdkDir 'makepri.exe'
foreach ($tool in $MakeAppx, $MakePri) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "$tool doesn't exist" }
}

# A fresh layout each time, so nothing from an earlier build rides along.
$Layout = Join-Path $OutDir 'layout'
if (Test-Path -LiteralPath $Layout) { Remove-Item -Recurse -Force -LiteralPath $Layout }
New-Item -ItemType Directory -Force -Path (Join-Path $Layout 'Assets') | Out-Null

foreach ($exe in 'upnext.exe', 'upnext-app.exe') {
    Copy-Item -LiteralPath (Join-Path $BinDir $exe) -Destination $Layout
}
foreach ($file in 'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD_PARTY_NOTICES.txt') {
    Copy-Item -LiteralPath (Join-Path $Root $file) -Destination $Layout
}
Copy-Item -Path (Join-Path $Source 'Assets\*.png') -Destination (Join-Path $Layout 'Assets')
[IO.File]::WriteAllText((Join-Path $Layout $Marker),
    "UpNext installed from its Windows package (MSIX). UpNext reads this file to know.`r`n",
    (New-Object Text.UTF8Encoding $false))

# The values go into XML attributes and text, so they're escaped for it.
$manifest = [IO.File]::ReadAllText((Join-Path $Source 'AppxManifest.xml'))
$values = @{
    '@IDENTITY_NAME@' = $IdentityName
    '@PUBLISHER@' = $Publisher
    '@PUBLISHER_DISPLAY_NAME@' = $PublisherDisplayName
    '@DISPLAY_NAME@' = $DisplayName
    '@VERSION@' = $PackageVersion
}
foreach ($key in $values.Keys) {
    $manifest = $manifest.Replace($key, [Security.SecurityElement]::Escape($values[$key]))
}
if ($manifest -match '@[A-Z_]+@') { throw "The manifest still has $($Matches[0]) in it" }
[IO.File]::WriteAllText((Join-Path $Layout 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding $false))

# resources.pri tells Windows which picture to use at each size and scale
# (the .scale-, .targetsize- and _altform-unplated names). Its
# configuration is kept out of the layout, which MakePri indexes whole.
$Obj = Join-Path $OutDir 'obj'
New-Item -ItemType Directory -Force -Path $Obj | Out-Null
$PriConfig = Join-Path $Obj 'priconfig.xml'
Invoke-Checked $MakePri @('createconfig', '/cf', $PriConfig, '/dq', 'en-US', '/pv', '10.0.0', '/o')
Invoke-Checked $MakePri @(
    'new', '/pr', $Layout, '/cf', $PriConfig, '/mn', (Join-Path $Layout 'AppxManifest.xml'),
    '/of', (Join-Path $Layout 'resources.pri'), '/o'
)
Remove-Item -Recurse -Force -LiteralPath $Obj

$Package = Join-Path $OutDir $PackageName
Write-Host "Building $PackageName $PackageVersion for $IdentityName from $BinDir"
# MakeAppx checks the manifest against the SDK's schemas as it packs.
Invoke-Checked $MakeAppx @('pack', '/d', $Layout, '/p', $Package, '/o')

# The same form as the MSI's and dist's checksums.
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Package).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$Package.sha256", "$hash *$PackageName`n", (New-Object Text.UTF8Encoding $false))
Write-Host "Wrote $Package"
Write-Host "SHA-256 $hash"
