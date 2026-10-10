<#
.SYNOPSIS
  Regenerates every icon asset from the 512x512 master assets/icon/hyperstack.png.

.DESCRIPTION
  Writes, under assets/icon/:
    hyperstack.ico            Windows executable and window icon (16..256; 256 stored as PNG,
                              smaller sizes as BMP for compatibility with older shells)
    hyperstack-<size>.png     512, 256, 128, 64, 48, 32, 16 - window icon (embedded in the
                              binary), launcher/About, README, Linux hicolor sizes
    hyperstack.icns           macOS bundle icon, only when `iconutil` exists (macOS)

  Requires ImageMagick 7 (`magick` on PATH). Run after changing the master PNG
  and commit the results; build.rs and the app embed these files, nothing is
  generated at build time.
#>
param([string]$Master = "assets/icon/hyperstack.png")

$ErrorActionPreference = "Stop"
if (-not (Get-Command magick -ErrorAction SilentlyContinue)) { throw "ImageMagick 7 (magick) is not on PATH." }
if (-not (Test-Path $Master)) { throw "Master image $Master not found." }
$dir = Split-Path $Master -Parent

$info = (magick identify -format "%w %h %m" $Master) -split " "
if ($info[0] -ne "512" -or $info[1] -ne "512") { throw "Master must be 512x512, got $($info[0])x$($info[1])." }

foreach ($s in 512, 256, 128, 64, 48, 32, 16) {
  magick $Master -resize "${s}x${s}" -filter Lanczos "$dir/hyperstack-$s.png"
  if ($LASTEXITCODE -ne 0) { throw "magick failed for size $s" }
}
magick $Master -define icon:auto-resize=256,128,64,48,32,16 "$dir/hyperstack.ico"
if ($LASTEXITCODE -ne 0) { throw "magick failed writing the .ico" }

if (Get-Command iconutil -ErrorAction SilentlyContinue) {
  $set = Join-Path $dir "hyperstack.iconset"
  New-Item -ItemType Directory -Force $set | Out-Null
  foreach ($pair in @(@(16,"16x16"),@(32,"16x16@2x"),@(32,"32x32"),@(64,"32x32@2x"),@(128,"128x128"),@(256,"128x128@2x"),@(256,"256x256"),@(512,"256x256@2x"),@(512,"512x512"))) {
    Copy-Item "$dir/hyperstack-$($pair[0]).png" "$set/icon_$($pair[1]).png"
  }
  iconutil -c icns $set -o "$dir/hyperstack.icns"
  Remove-Item $set -Recurse -Force
}

Get-ChildItem $dir -File | ForEach-Object { Write-Host ("  {0,-24} {1,8:N0} bytes" -f $_.Name, $_.Length) }
magick identify "$dir/hyperstack.ico" | ForEach-Object { Write-Host "  $_" }
exit 0
