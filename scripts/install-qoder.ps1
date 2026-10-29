param([string]$Archive, [string]$Destination, [string]$Sha256)
$ErrorActionPreference = 'Stop'
$hasher = [Security.Cryptography.SHA256]::Create()
$stream = [IO.File]::OpenRead($Archive)
try { $actual = [BitConverter]::ToString($hasher.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
finally { $stream.Dispose(); $hasher.Dispose() }
if ($actual -ne $Sha256) {
    throw 'Qoder 官方安装包 SHA-256 校验失败'
}
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($Archive)
try {
    $binary = $zip.GetEntry('qodercli.exe')
    if (-not $binary) { throw '官方安装包缺少 qodercli.exe' }
    if ($binary.Length -gt 400MB) { throw '程序超过大小限制' }
    [IO.Compression.ZipFileExtensions]::ExtractToFile($binary, $Destination, $false)
} finally { $zip.Dispose() }
