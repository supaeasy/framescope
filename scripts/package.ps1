<#
.SYNOPSIS
  Erzeugt das Portable-ZIP (EXE, benoetigte FFmpeg-DLLs, README, Lizenzen) samt SHA256-Datei.
.EXAMPLE
  ./scripts/package.ps1 -Version 0.1.0 -FfmpegDir C:\dev\ffmpeg9\ffmpeg-n9.0-latest-win64-lgpl-shared-9.0
#>
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $FfmpegDir,
    [string] $Exe = "target/release/framescope.exe",
    [string] $OutDir = "dist"
)
$ErrorActionPreference = "Stop"
$Version = $Version.TrimStart('v')
$name = "framescope-v$Version-windows-x64-portable"

if (-not (Test-Path $Exe)) { throw "EXE nicht gefunden: $Exe (zuerst 'cargo build --release')" }
$bin = Join-Path $FfmpegDir "bin"
if (-not (Test-Path $bin)) { throw "FFmpeg-bin-Ordner nicht gefunden: $bin" }

$stage = Join-Path $OutDir $name
if (Test-Path $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage "licenses") | Out-Null
New-Item -ItemType Directory -Force $OutDir | Out-Null

Copy-Item $Exe $stage

# Nur die DLLs mitliefern, die die EXE wirklich importiert (z. B. ohne avfilter/avdevice).
$exeText = [System.Text.Encoding]::ASCII.GetString([System.IO.File]::ReadAllBytes($Exe))
$dlls = Get-ChildItem (Join-Path $bin "*.dll") | Where-Object { $exeText.Contains($_.Name) }
if (-not $dlls) { throw "Keine FFmpeg-DLLs gefunden, die von der EXE importiert werden." }
$dlls | ForEach-Object { Copy-Item $_.FullName $stage }

foreach ($f in "README.md", "LICENSE", "CHANGELOG.md", "THIRD-PARTY-LICENSES.md") { Copy-Item $f $stage }
$ffLicense = Join-Path $FfmpegDir "LICENSE.txt"
if (-not (Test-Path $ffLicense)) { throw "FFmpeg-LICENSE.txt fehlt in $FfmpegDir" }
Copy-Item $ffLicense (Join-Path $stage "licenses/FFmpeg-LICENSE.txt")

$zip = Join-Path $OutDir "$name.zip"
if (Test-Path $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip -CompressionLevel Optimal

$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash *$name.zip" | Set-Content -Encoding ascii "$zip.sha256"

# Smoke-Test: aus dem entpackten ZIP starten, mit minimalem PATH (DLLs muessen daneben liegen).
$check = Join-Path ([System.IO.Path]::GetTempPath()) "$name-check"
if (Test-Path $check) { Remove-Item -LiteralPath $check -Recurse -Force }
Expand-Archive $zip $check
$savedPath = $env:PATH
$env:PATH = "$env:SystemRoot\system32;$env:SystemRoot"
try {
    $p = Start-Process (Join-Path $check "framescope.exe") -ArgumentList "--version" -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "Smoke-Test fehlgeschlagen (Exit-Code $($p.ExitCode)) - fehlen DLLs im ZIP?" }
} finally {
    $env:PATH = $savedPath
    Remove-Item -LiteralPath $check -Recurse -Force
}

$size = [math]::Round((Get-Item $zip).Length / 1MB, 1)
Write-Host "OK: $zip ($size MB)"
Write-Host "DLLs: $(($dlls | ForEach-Object Name) -join ', ')"
Write-Host "SHA256: $hash"
if ($env:GITHUB_OUTPUT) {
    "zip=$zip" | Out-File -Append -Encoding utf8 $env:GITHUB_OUTPUT
    "sha=$zip.sha256" | Out-File -Append -Encoding utf8 $env:GITHUB_OUTPUT
}
