# Installs the latest deep release for the current user.
#   irm https://github.com/B33BMO/deep/releases/latest/download/install.ps1 | iex
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
$url = "https://github.com/B33BMO/deep/releases/latest/download/deep-windows-$arch.exe"
$dir = Join-Path $env:LOCALAPPDATA 'Programs\deep'
$exe = Join-Path $dir 'deep.exe'

New-Item -ItemType Directory -Force -Path $dir | Out-Null
Write-Host "Downloading $url"
try {
    Invoke-WebRequest -Uri $url -OutFile "$exe.new" -UseBasicParsing
    Move-Item -Force "$exe.new" $exe
} catch {
    Remove-Item -ErrorAction SilentlyContinue "$exe.new"
    throw "Install failed (is deep still running? close it and try again): $_"
}

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$parts = @($userPath -split ';' | Where-Object { $_ })
if ($parts -notcontains $dir) {
    [Environment]::SetEnvironmentVariable('Path', (($parts + $dir) -join ';'), 'User')
    $env:Path = "$env:Path;$dir"
    Write-Host "Added $dir to your PATH"
}

Write-Host ""
& $exe --version
Write-Host "Installed to $exe"
Write-Host "Run 'deep' (open a new terminal if it isn't found). Run as Administrator to see and kill system processes."
