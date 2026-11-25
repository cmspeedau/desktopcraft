# Isolated native candidate build. No launch, installation, packaging, publication or upload.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
$oldDll = $env:DESKTOPCRAFT_NATIVE_FILTER_DLL
$oldEncodedFlags = $env:CARGO_ENCODED_RUSTFLAGS
try {
    if ($env:OS -ne 'Windows_NT') { throw 'Windows required.' }
    function Invoke-Checked {
        param([string]$Program,[string[]]$Arguments)
        $result = & $Program @Arguments
        if ($LASTEXITCODE -ne 0) { throw "$Program failed: $($Arguments -join ' ')" }
        return $result
    }
    $dirty = Invoke-Checked git @('status','--porcelain','--untracked-files=all')
    if ($dirty) { throw 'Clean committed worktree required.' }
    $commit = (Invoke-Checked git @('rev-parse','HEAD')).Trim()
    $tree = (Invoke-Checked git @('rev-parse','HEAD^{tree}')).Trim()
    # Record effective toolchain and flag overrides; refuse overrides that could
    # replace the compiler, linker or injected flags in this controlled build.
    foreach ($name in @('RUSTC','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER','CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS','CARGO_BUILD_RUSTFLAGS')) {
        if ([Environment]::GetEnvironmentVariable($name)) { throw "Unset $name for the controlled build." }
    }
    $rust = (Invoke-Checked rustc @('-vV')) -join [Environment]::NewLine
    $cargo = (Invoke-Checked cargo @('-V')) -join [Environment]::NewLine
    # Keep build paths and the builder username out of public binaries.
    $homePath = [Environment]::GetFolderPath('UserProfile')
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $homePath '.cargo' }
    $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $homePath '.rustup' }
    $sysroot = (Invoke-Checked rustc @('--print','sysroot')).Trim()
    $flags = @('-C','target-feature=+crt-static')
    foreach ($pair in @(@($homePath,'/user'),@($cargoHome,'/cargo'),@($rustupHome,'/rustup'),@($sysroot,'/rustc'),@($root,'/desktopcraft'))) {
        if ($pair[0]) {
            $flags += @('--remap-path-prefix', ($pair[0] + '=' + $pair[1]))
            $flags += @('--remap-path-prefix', ($pair[0].Replace('\','/') + '=' + $pair[1]))
        }
    }
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
    $triple = 'x86_64-pc-windows-msvc'
    $out = Join-Path ([IO.Path]::GetTempPath()) ('desktopcraft-provenance-' + [Guid]::NewGuid().ToString('N'))
    $native = Join-Path $out 'native'
    $app = Join-Path $out 'app'
    Remove-Item Env:DESKTOPCRAFT_NATIVE_FILTER_DLL -ErrorAction SilentlyContinue
    Invoke-Checked cargo @('build','--locked','--release','--target',$triple,'--manifest-path','native-filter/Cargo.toml','--features','native-fast-tabs','--target-dir',$native) | Out-Host
    $dll = Join-Path $native "$triple/release/desktopcraft_native_filter.dll"
    if (!(Test-Path -LiteralPath $dll -PathType Leaf)) { throw 'Fresh native DLL missing.' }
    $before = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLowerInvariant()
    $env:DESKTOPCRAFT_NATIVE_FILTER_DLL = $dll
    Invoke-Checked cargo @('build','--locked','--release','--target',$triple,'--bin','desktopcraft','--features','native-fast-tabs','--target-dir',$app) | Out-Host
    $exe = Join-Path $app "$triple/release/desktopcraft.exe"
    $pair = & (Join-Path $PSScriptRoot 'verify-native-resource.ps1') -Executable $exe -NativeDll $dll
    if ($pair.native_dll_sha256 -ne $before) { throw 'Native input changed during build.' }
    if ((Invoke-Checked git @('rev-parse','HEAD')).Trim() -ne $commit -or (Invoke-Checked git @('status','--porcelain','--untracked-files=all'))) {
        throw 'Source changed during build.'
    }
    $inputs = [ordered]@{}
    foreach ($p in @('Cargo.toml','Cargo.lock','native-filter/Cargo.toml','native-filter/Cargo.lock','build.rs','.cargo/config.toml','assets/desktopcraft.ico','assets/desktopcraft-16.rgba','assets/desktopcraft-32.rgba','assets/desktopcraft-64.rgba')) {
        $inputs[$p] = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $record = [ordered]@{
        schema = 1
        source_commit = $commit
        source_tree = $tree
        target = $triple
        features = 'native-fast-tabs'
        profile = 'release'
        rustc = $rust
        cargo = $cargo
        inputs = $inputs
        artifact = $pair
        scope = 'Local build evidence; unsigned, not legal clearance or live acceptance.'
    }
    $manifest = Join-Path $out 'build-provenance.json'
    $record | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $manifest -Encoding UTF8
    Write-Host "Verified native candidate (not executed): $exe"
    Write-Output $manifest
} finally {
    $env:DESKTOPCRAFT_NATIVE_FILTER_DLL = $oldDll
    $env:CARGO_ENCODED_RUSTFLAGS = $oldEncodedFlags
    Pop-Location
}
