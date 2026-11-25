# Read PE resources without executing the EXE or DLL.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$NativeDll
)
$ErrorActionPreference = 'Stop'
function Assert-X64Pe([string]$Path, [bool]$Dll) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = New-Object IO.BinaryReader($stream)
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) { throw "Invalid DOS header: $Path" }
        $stream.Position = 60
        $pe = $reader.ReadUInt32()
        if ([long]$pe + 26 -gt $stream.Length) { throw "Invalid PE offset: $Path" }
        $stream.Position = $pe
        if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) { throw "Expected x64 PE: $Path" }
        $stream.Position = [long]$pe + 22
        $characteristics = $reader.ReadUInt16()
        if ((($characteristics -band 0x2000) -ne 0) -ne $Dll) { throw "Unexpected PE DLL flag: $Path" }
        if ($reader.ReadUInt16() -ne 0x20b) { throw "Expected PE32+: $Path" }
    } finally { $stream.Dispose() }
}
$exe = (Resolve-Path -LiteralPath $Executable).Path
$dll = (Resolve-Path -LiteralPath $NativeDll).Path
Assert-X64Pe $exe $false
Assert-X64Pe $dll $true
if (-not ('DesktopCraftVerification.ResourceReader' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace DesktopCraftVerification {
    public static class ResourceReader {
        [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
        static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
        [DllImport("kernel32.dll", EntryPoint="FindResourceW", SetLastError=true)]
        static extern IntPtr FindResource(IntPtr module, IntPtr name, IntPtr type);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern uint SizeofResource(IntPtr module, IntPtr resource);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
        [DllImport("kernel32.dll")]
        static extern IntPtr LockResource(IntPtr data);
        [DllImport("kernel32.dll", SetLastError=true)]
        static extern bool FreeLibrary(IntPtr module);
        public static byte[] ReadNativeResource(string path) {
            // LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE.
            // No imports resolved, entry point or DLL initialization executed.
            IntPtr m = LoadLibraryEx(path, IntPtr.Zero, 0x22);
            if (m == IntPtr.Zero) throw new Win32Exception();
            try {
                IntPtr r = FindResource(m, new IntPtr(200), new IntPtr(10));
                if (r == IntPtr.Zero) throw new Win32Exception();
                uint size = SizeofResource(m, r);
                if (size == 0 || size > 64 * 1024 * 1024) throw new InvalidOperationException("Invalid native resource size");
                IntPtr h = LoadResource(m, r);
                if (h == IntPtr.Zero) throw new Win32Exception();
                IntPtr p = LockResource(h);
                if (p == IntPtr.Zero) throw new InvalidOperationException("Resource lock failed");
                byte[] result = new byte[(int)size];
                Marshal.Copy(p, result, 0, result.Length);
                return result;
            } finally { FreeLibrary(m); }
        }
    }
}
'@
}
$embedded = [DesktopCraftVerification.ResourceReader]::ReadNativeResource($exe)
$hasher = [Security.Cryptography.SHA256]::Create()
try { $embeddedHash = ([BitConverter]::ToString($hasher.ComputeHash($embedded))).Replace('-','').ToLowerInvariant() }
finally { $hasher.Dispose() }
$dllHash = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLowerInvariant()
if ($embeddedHash -ne $dllHash -or $embedded.Length -ne (Get-Item -LiteralPath $dll).Length) {
    throw 'Embedded RT_RCDATA #200 does not match the expected native DLL.'
}
[pscustomobject]@{
    executable = $exe
    executable_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    native_dll = $dll
    native_dll_sha256 = $dllHash
    embedded_sha256 = $embeddedHash
    embedded_bytes = $embedded.Length
    machine = 'x64'
}
