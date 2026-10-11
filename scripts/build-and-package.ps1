<#
.SYNOPSIS
  Builds HyperStack.exe and assembles everything a release publishes.

.DESCRIPTION
  The in-app updater (hs-core::update) reads exactly these asset names, so they
  are a contract, not a convention (docs/09-ci-cd-and-updates.md section 4.3):

    HyperStack.exe                          bare executable, downloaded in place
    HyperStack-<version>-windows-x64.zip    manual update: exe + README.txt + LICENSE + notes
    manifest.json                           version, channel, assets with size and sha256
    manifest.json.minisig                   Ed25519 signature of manifest.json (minisign format)
    SHA256SUMS.txt                          for humans and `sha256sum -c`
    CHANGELOG-<version>.md                  the release notes, also the release body

  The SIGNED MANIFEST is the trust anchor: the app refuses a release whose
  manifest signature or hashes do not verify. Signing uses rsign2 (a minisign
  implementation in Rust, `cargo install rsign2`) with the secret key taken from
  $env:HS_MINISIGN_KEY (the key file's contents, generated with
  `rsign generate --unencrypted`; a `-W` key still prompts for a password and
  hangs a runner - see keys/README.md). The matching public key is committed as
  keys/minisign.pub and embedded in the app. Use -SkipSign only for local
  packaging; the release workflow never does.

  Run scripts/update-version.ps1 first: the binary's version comes from the
  workspace Cargo.toml at build time, and this script refuses to package a
  binary that reports a different version than the one being released.
#>
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [ValidateSet("stable", "beta")][string]$Channel = "stable",
  [string]$Target = "x86_64-pc-windows-msvc",
  [string]$NotesFile = "release/notes.md",
  [string]$PublicKey = "keys/minisign.pub",
  # Base web URL of the forge this release is being published to; the manifest's
  # asset URLs, notes_url and the zip's README.txt are built from it. The GitHub
  # pipeline MUST pass this: the repository has GitHub as a *push* remote only,
  # so `git remote get-url origin` still answers with the Forgejo URL there.
  [string]$RepoUrl = "",
  # Oldest version that can update in place to this one; older copies are sent
  # to the zip. Bump only when the updater protocol changes.
  [string]$MinSupportedVersion = "0.1.0",
  # Version of the on-disk data format this build writes (see docs/05).
  [int]$DataSchema = 1,
  [switch]$SkipBuild,
  [switch]$SkipSign
)

$ErrorActionPreference = "Stop"

function Invoke-Checked([string]$what, [scriptblock]$block) {
  & $block
  if ($LASTEXITCODE -ne 0) { throw "$what failed with exit code $LASTEXITCODE." }
}

if ($Version -notmatch '^\d+\.\d+\.\d+(-beta\.\d+)?$') { throw "Version '$Version' is not a release version." }
if (-not (Test-Path $NotesFile)) { throw "Release notes $NotesFile are missing - run scripts/next-release.ps1 first." }
$notes = (Get-Content $NotesFile -Raw).Trim()
if (-not $notes) { throw "Release notes $NotesFile are empty." }

# Where this release will live. Needed before the zip's README.txt is written.
if (-not $RepoUrl) {
  $origin = (git remote get-url origin 2>$null)
  $RepoUrl = if ($origin) { ($origin -replace '\.git$', '') } else { "https://git.zacx.net/owner/hyper-stack" }
}
$RepoUrl = $RepoUrl.TrimEnd('/')
if ($RepoUrl -notmatch '^https://') { throw "RepoUrl '$RepoUrl' must be an https URL." }
$forgeHost = ([uri]$RepoUrl).Host

# --- Build ----------------------------------------------------------------------
if (-not $SkipBuild) {
  Invoke-Checked "cargo build" { cargo build --release --locked -p hyperstack --target $Target }
}
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
$built = Join-Path $targetDir "$Target/release/HyperStack.exe"
if (-not (Test-Path $built)) { throw "No binary at $built after the build." }

# The app compares release tags against its own CARGO_PKG_VERSION; a binary
# built from the wrong Cargo.toml would offer the same update for ever.
# The exe is a GUI-subsystem binary, so `&` would not wait for it; Start-Process does.
$versionOut = Join-Path ([System.IO.Path]::GetTempPath()) ("hs-version-" + [guid]::NewGuid() + ".txt")
$p = Start-Process -FilePath $built -ArgumentList @('--version') -Wait -PassThru -NoNewWindow -RedirectStandardOutput $versionOut
$reported = (Get-Content $versionOut -Raw -ErrorAction SilentlyContinue | Out-String).Trim()
Remove-Item $versionOut -Force -ErrorAction SilentlyContinue
if ($p.ExitCode -ne 0) { throw "HyperStack.exe --version exited with $($p.ExitCode): $reported" }
if ($reported -notmatch [regex]::Escape($Version)) { throw "The binary reports '$reported' but $Version is being released." }

# --- Assemble release/dist -------------------------------------------------------
$dist = "release/dist"
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory -Path $dist -Force | Out-Null

$exeName  = "HyperStack.exe"
$zipName  = "HyperStack-$Version-windows-x64.zip"
$noteName = "CHANGELOG-$Version.md"

Copy-Item $built "$dist/$exeName"
"# HyperStack $Version`n`n$notes`n" | Set-Content "$dist/$noteName" -Encoding utf8

$staging = "release/zip-staging"
if (Test-Path $staging) { Remove-Item $staging -Recurse -Force }
New-Item -ItemType Directory -Path $staging -Force | Out-Null
Copy-Item $built "$staging/$exeName"
Copy-Item "$dist/$noteName" $staging
if (Test-Path LICENSE) { Copy-Item LICENSE $staging }
@"
HyperStack $Version ($Channel)

Portable: unzip anywhere writable and run HyperStack.exe. Everything the app
writes - settings, workspaces, the encrypted secret vault, history, logs -
goes into a HyperStackData folder created next to the executable. Nothing is
written to the registry or to your user profile. To move or back up the app,
copy the whole folder. To reset it, delete HyperStackData.

Updating: Settings > Updates checks $forgeHost - where this copy came from -
for newer releases and can install them in place. To update by hand, replace
HyperStack.exe with the one from a newer zip; keep your HyperStackData folder.

Verifying this download: SHA256SUMS.txt on the release page lists the hash of
every file, and manifest.json is signed (minisign) with the key printed in
Settings > About.

Release notes: see $noteName
Project: $RepoUrl
"@ | Set-Content "$staging/README.txt" -Encoding utf8
Compress-Archive -Path "$staging/*" -DestinationPath "$dist/$zipName"
Remove-Item $staging -Recurse -Force

# --- manifest.json ------------------------------------------------------------
# Forgejo and GitHub both serve release assets at
# <repo>/releases/download/<tag>/<asset>, so one layout covers both.
$tag = "v$Version"
$download = "$RepoUrl/releases/download/$tag"

function Asset-Entry([string]$name) {
  $file = Get-Item "$dist/$name"
  [ordered]@{
    name   = $name
    url    = "$download/$name"
    size   = $file.Length
    sha256 = (Get-FileHash $file.FullName -Algorithm SHA256).Hash.ToLower()
  }
}

$manifest = [ordered]@{
  schema                = 1
  product               = "HyperStack"
  version               = $Version
  channel               = $Channel
  published_at          = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  tag                   = $tag
  notes_url             = "$RepoUrl/releases/tag/$tag"
  min_supported_version = $MinSupportedVersion
  data_schema           = $DataSchema
  assets                = [ordered]@{
    "windows-x64"     = Asset-Entry $exeName
    "windows-x64-zip" = Asset-Entry $zipName
  }
}
($manifest | ConvertTo-Json -Depth 6) + "`n" | Set-Content "$dist/manifest.json" -Encoding utf8 -NoNewline

# --- Sign the manifest -----------------------------------------------------------
if ($SkipSign) {
  Write-Warning "Manifest NOT signed (-SkipSign). The app will refuse this release; local use only."
} else {
  if (-not $env:HS_MINISIGN_KEY) { throw "HS_MINISIGN_KEY is not set and -SkipSign was not given." }
  if (-not (Get-Command rsign -ErrorAction SilentlyContinue)) { throw "rsign (rsign2) is not installed: cargo install rsign2 --locked" }
  if (-not (Test-Path $PublicKey)) { throw "Public key $PublicKey is missing; it must be committed so the release can be verified." }
  $keyFile = Join-Path ([System.IO.Path]::GetTempPath()) ("hs-minisign-" + [guid]::NewGuid() + ".key")
  try {
    # A minisign secret key file is two lines: "untrusted comment: ..." and the
    # base64 key. Forge secrets are often pasted as the key line alone, or with
    # CRLF endings; normalise to the exact file layout rsign expects.
    $lines = @($env:HS_MINISIGN_KEY -split "`r?`n" | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne "" })
    if (-not $lines.Count) { throw "HS_MINISIGN_KEY is empty." }
    if ($lines[0] -notmatch '^untrusted comment:') { $lines = @("untrusted comment: minisign secret key") + $lines }
    if ($lines.Count -ne 2) { throw "HS_MINISIGN_KEY must be the two-line minisign secret key (comment line + key line); got $($lines.Count) lines." }
    [System.IO.File]::WriteAllText($keyFile, ($lines -join "`n") + "`n")
    # rsign asks for a password on the console (not stdin) when the secret key
    # is encrypted - including keys made with `rsign generate -W`. On a runner
    # that would hang for ever, so sign with a timeout and explain the fix.
    $rsign = (Get-Command rsign).Source
    $sig = Join-Path $dist "manifest.json.minisig"
    # Start-Process does not quote ArgumentList entries, so every path is wrapped
    # in quotes and the trusted comment contains no spaces.
    $q = { param($v) '"' + $v + '"' }
    $args = @('sign', '-s', (& $q $keyFile), '-x', (& $q $sig), '-t', "HyperStack-$Version-$Channel", (& $q (Join-Path $dist 'manifest.json')))
    $proc = Start-Process -FilePath $rsign -ArgumentList $args -PassThru -NoNewWindow
    if (-not $proc.WaitForExit(60000)) {
      $proc.Kill()
      throw "rsign sign did not finish in 60 s: the secret key in HS_MINISIGN_KEY needs a password. Regenerate it with 'rsign generate --unencrypted' (keys/README.md) and update the secret and keys/minisign.pub."
    }
    if ($proc.ExitCode -ne 0) { throw "rsign sign failed with exit code $($proc.ExitCode). If the message is 'Missing encoded key', the secret is not a minisign secret key; if it asks for a password, regenerate with 'rsign generate --unencrypted'." }
    Invoke-Checked "rsign verify" { rsign verify -p $PublicKey -x "$dist/manifest.json.minisig" "$dist/manifest.json" }
  } finally {
    if (Test-Path $keyFile) { Remove-Item $keyFile -Force }
  }
}

# --- SHA256SUMS.txt (sha256sum layout: hash, two spaces, name) --------------------
Get-ChildItem $dist -File | Where-Object { $_.Name -ne "SHA256SUMS.txt" } | Sort-Object Name | ForEach-Object {
  "$((Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.Name)"
} | Set-Content "$dist/SHA256SUMS.txt" -Encoding ascii

# --- Contract check -----------------------------------------------------------------
$required = @($exeName, $zipName, "manifest.json", "SHA256SUMS.txt", $noteName)
if (-not $SkipSign) { $required += "manifest.json.minisig" }
foreach ($name in $required) {
  if (-not (Test-Path "$dist/$name")) { throw "Packaging did not produce $name." }
}

Write-Host "Packaged HyperStack $Version ($Channel) for $Target"
Get-ChildItem $dist -File | ForEach-Object { Write-Host ("  {0,-44} {1,12:N0} bytes" -f $_.Name, $_.Length) }
exit 0
