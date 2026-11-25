# Local candidate packaging only; does not authorize publication or establish legal clearance.
param(
    [Parameter(Mandatory=$true)][string]$NativeDll,
    [Parameter(Mandatory=$true)][string]$ExpectedSha256,
    [Parameter(Mandatory=$true)][string]$Destination,
    [Parameter(Mandatory=$true)][string]$Executable
)
$ErrorActionPreference = 'Stop'
$project = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$version = (Select-String -LiteralPath (Join-Path $project 'Cargo.toml') -Pattern '^version\s*=\s*"(\d+\.\d+\.\d+)"' | Select-Object -First 1).Matches.Groups[1].Value
if (-not $version) { throw 'Missing package version.' }
$release = (Resolve-Path -LiteralPath $Executable).Path
$verified = & (Join-Path $project 'tools/verify-native-resource.ps1') -Executable $release -NativeDll $NativeDll
if ($verified.executable_sha256 -ne $ExpectedSha256) { throw 'Candidate hash differs from approved checkpoint.' }
$info = (Get-Item -LiteralPath $release).VersionInfo
if ($info.FileVersion -ne "$version.0" -or $info.ProductVersion -ne "$version.0") { throw 'Version mismatch.' }
if (-not [IO.Path]::IsPathRooted($Destination)) { throw 'Use a new absolute local output directory.' }
if (Test-Path -LiteralPath $Destination) { throw 'Destination already exists.' }
New-Item -ItemType Directory -Path $Destination | Out-Null
$filename = "DesktopCraft-v$version-Setup-x64.exe"
$installer = Join-Path $Destination $filename
Copy-Item -LiteralPath $release -Destination $installer
$sha = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
if ($sha -ne $ExpectedSha256) { throw 'Installer copy hash mismatch; do not distribute.' }
foreach ($p in @('THIRD-PARTY-NOTICES.md','LICENSE','NOTICE.md','third-party-licenses')) { Copy-Item -LiteralPath (Join-Path $project $p) -Destination $Destination -Recurse }
Copy-Item -LiteralPath (Join-Path $project 'assets/README.md') -Destination (Join-Path $Destination 'ARTWORK-NOTICE.md')
[IO.File]::WriteAllText((Join-Path $Destination 'SHA256SUMS.txt'), "$sha  $filename" + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
Write-Output "Local unsigned installer: $installer"
Write-Output "SHA-256: $sha"
