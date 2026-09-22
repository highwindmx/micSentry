# One-click build for MicTrayRs (Windows). Pure ASCII on purpose (encoding safety).
#
# Usage:
#   double-click build.cmd
#       -> MSVC host target (default). Uses the installed rustup toolchain plus the
#          Visual Studio linker; no extra download.
#   powershell -ExecutionPolicy Bypass -File .\build.ps1 -Gnu
#       -> MinGW-w64 / x86_64-pc-windows-gnu target.
#          Downloads the toolchain on first use (~150 MB).
param(
    [switch]$Gnu
)
$ErrorActionPreference = 'Stop'

# A native program's non-zero exit code does NOT raise a PowerShell exception,
# so $LASTEXITCODE must be checked explicitly. Otherwise a failed compile would
# still print "Build complete" (misleading).
function Invoke-Cargo([string[]]$CargoArgs) {
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host ''
        Write-Host ('BUILD FAILED (cargo exit code ' + $LASTEXITCODE + ').')
        exit $LASTEXITCODE
    }
}

if ($Gnu) {
    $mingwUrl = 'https://github.com/brechtsanders/winlibs_mingw/releases/download/16.2.0posix-14.0.0-msvcrt-r1/winlibs-x86_64-posix-seh-gcc-16.2.0-mingw-w64msvcrt-14.0.0-r1.zip'
    $toolchain = Join-Path $PSScriptRoot 'toolchain'
    # A USABLE MinGW needs the target sysroot (crt2.o, libkernel32.a, ...), not just gcc.exe.
    # Checking only for gcc.exe is what let a truncated download go unnoticed.
    $sysrootLib = Join-Path $toolchain 'mingw64\x86_64-w64-mingw32\lib\libkernel32.a'

    if (-not (Test-Path $sysrootLib)) {
        Write-Host 'MinGW-w64 missing or incomplete; downloading (~150 MB, first run only)...'
        foreach ($p in @((Join-Path $toolchain 'mingw64'), (Join-Path $toolchain 'mingw64.zip'))) {
            if (Test-Path $p) { Remove-Item -Recurse -Force $p }
        }
        New-Item -ItemType Directory -Force -Path $toolchain | Out-Null
        $zip = Join-Path $toolchain 'mingw64.zip'
        # curl.exe follows the GitHub release redirect to the CDN far more reliably
        # than Invoke-WebRequest for large binaries (a truncated zip was the old bug).
        & curl.exe -L --fail --retry 3 -o $zip $mingwUrl
        if ($LASTEXITCODE -ne 0) {
            Write-Host 'MinGW-w64 download failed.'
            exit 1
        }
        Expand-Archive -Path $zip -DestinationPath $toolchain -Force
        Remove-Item $zip
        if (-not (Test-Path $sysrootLib)) {
            Write-Host 'MinGW-w64 extraction is incomplete: still no target sysroot. Aborting.'
            exit 1
        }
    }

    $env:PATH = (Join-Path $toolchain 'mingw64\bin') + ';' + $env:PATH
    & rustup target add x86_64-pc-windows-gnu
    Invoke-Cargo -CargoArgs @('build', '--release', '--target', 'x86_64-pc-windows-gnu')
    $exe = Join-Path (Join-Path 'target' 'x86_64-pc-windows-gnu') (Join-Path 'release' 'MicTrayRs.exe')
} else {
    Invoke-Cargo -CargoArgs @('build', '--release')
    $exe = Join-Path 'target' (Join-Path 'release' 'MicTrayRs.exe')
}

Write-Host ''
Write-Host 'Build complete:'
Write-Host ('  ' + (Resolve-Path $exe))
Write-Host ('  size: ' + (Get-Item $exe).Length + ' bytes')
