# Generate MicTray single-file self-contained exe -> dist/MicTray.exe
$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
dotnet publish MicTray.csproj -c Release -r win-x64 --self-contained `
  -p:PublishSingleFile=true `
  -p:IncludeNativeLibrariesForSelfExtract=true `
  -p:EnableCompressionInSingleFile=true `
  -o dist
Write-Host "Done -> dist/MicTray.exe"
Pop-Location
