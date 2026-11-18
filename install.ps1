# srvr installer for Windows — https://github.com/flaxodotdev/srvr
#   irm https://raw.githubusercontent.com/flaxodotdev/srvr/main/install.ps1 | iex
# Env: SRVR_VERSION (default: latest), SRVR_INSTALL_DIR (default: %LOCALAPPDATA%\srvr\bin)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'flaxodotdev/srvr'
$version = if ($env:SRVR_VERSION) { $env:SRVR_VERSION } else { 'latest' }
$installDir = if ($env:SRVR_INSTALL_DIR) { $env:SRVR_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'srvr\bin' }

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    'AMD64' { 'x86_64' }
    'ARM64' { 'aarch64' }
    default { throw "unsupported architecture: $($env:PROCESSOR_ARCHITECTURE)" }
}
$asset = "srvr-$arch-pc-windows-msvc.zip"
$base = if ($version -eq 'latest') { "https://github.com/$repo/releases/latest/download" } else { "https://github.com/$repo/releases/download/$version" }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("srvr-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "▲ srvr downloading $asset ($version)"
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -Uri "$base/$asset" -OutFile $zip -UseBasicParsing

    try {
        $sumFile = Join-Path $tmp "$asset.sha256"
        Invoke-WebRequest -Uri "$base/$asset.sha256" -OutFile $sumFile -UseBasicParsing
        $expected = ((Get-Content $sumFile -Raw).Trim() -split '\s+')[0].ToLower()
        $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
        if ($expected -ne $actual) { throw "checksum mismatch for $asset" }
        Write-Host "▲ srvr checksum ok"
    } catch [System.Net.WebException] { }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    Copy-Item (Join-Path $tmp 'srvr.exe') (Join-Path $installDir 'srvr.exe') -Force

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not ($userPath -split ';' | Where-Object { $_ -eq $installDir })) {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$installDir", 'User')
        $env:Path = "$env:Path;$installDir"
        Write-Host "▲ srvr added $installDir to your PATH (restart your terminal)"
    }
    $v = & (Join-Path $installDir 'srvr.exe') --version
    Write-Host "▲ srvr installed $v to $installDir — run: srvr"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
