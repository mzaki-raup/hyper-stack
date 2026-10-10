<#
.SYNOPSIS
  Sets the workspace version in Cargo.toml and Cargo.lock, and moves the
  release notes into CHANGELOG.md.

.DESCRIPTION
  HyperStack is a Cargo workspace: the single version lives under
  [workspace.package] in the root Cargo.toml and every member crate declares
  `version.workspace = true`. Cargo.lock records the version once per member,
  so every member entry is updated; the release build runs with --locked and
  would fail if any of them still said the old version.

  This only edits files - committing and tagging is the workflow's job, so the
  script stays runnable locally without touching git.

  The version matters beyond packaging: the app compares a release's tag
  against its own CARGO_PKG_VERSION, so the binary must be built after this
  runs or it would offer the same update for ever.
#>
param(
  [Parameter(Mandatory = $true)][string]$Version,
  # When given, its contents become the "## <version>" section at the top of
  # CHANGELOG.md and the Unreleased section is emptied.
  [string]$NotesFile = ""
)

$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+(-beta\.\d+)?$') {
  throw "Version '$Version' is not MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-beta.N."
}

# --- Cargo.toml: the version line inside [workspace.package] only --------------
$toml = Get-Content Cargo.toml
$start = -1
for ($i = 0; $i -lt $toml.Count; $i++) { if ($toml[$i] -match '^\s*\[workspace\.package\]\s*$') { $start = $i; break } }
if ($start -lt 0) { throw "Cargo.toml has no [workspace.package] table; the version must live there." }
$versionLine = -1
for ($i = $start + 1; $i -lt $toml.Count; $i++) {
  if ($toml[$i] -match '^\s*\[') { break }                      # next table
  if ($toml[$i] -match '^\s*version\s*=') { $versionLine = $i; break }
}
if ($versionLine -lt 0) { throw "No 'version =' line inside [workspace.package]." }
$toml[$versionLine] = "version = `"$Version`""
$toml | Set-Content Cargo.toml -Encoding utf8

# --- Cargo.lock: every workspace member's own entry ----------------------------
$metadata = cargo metadata --no-deps --format-version 1 --locked 2>$null | ConvertFrom-Json
if (-not $metadata) {
  # --locked can refuse once Cargo.toml changed; read the member list without it.
  $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
}
$members = @($metadata.packages | ForEach-Object { $_.name })
if (-not $members.Count) { throw "cargo metadata returned no workspace members." }

$lock = Get-Content Cargo.lock
$updated = 0
for ($i = 0; $i -lt $lock.Count - 1; $i++) {
  if ($lock[$i] -match '^name = "([^"]+)"$' -and $members -contains $Matches[1]) {
    if ($lock[$i + 1] -notmatch '^version\s*=') { throw "Cargo.lock line $($i + 2) is not the version line for $($Matches[1])." }
    $lock[$i + 1] = "version = `"$Version`""
    $updated++
  }
}
if ($updated -ne $members.Count) {
  throw "Updated $updated Cargo.lock entries but the workspace has $($members.Count) members ($($members -join ', '))."
}
$lock | Set-Content Cargo.lock -Encoding utf8

# --- CHANGELOG.md --------------------------------------------------------------
if ($NotesFile) {
  if (-not (Test-Path $NotesFile)) { throw "Notes file $NotesFile does not exist." }
  $notes = (Get-Content $NotesFile -Raw).TrimEnd()
  $changelog = if (Test-Path CHANGELOG.md) { Get-Content CHANGELOG.md -Raw } else { "# Changelog`n`n## Unreleased`n" }
  $date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-dd")
  $section = "## Unreleased`n`n- (nothing yet)`n`n## $Version — $date`n`n$notes`n"
  # The Unreleased section runs from its heading to the next "## " heading (or
  # the end of the file); it is replaced by an empty Unreleased plus the new
  # version section. A scriptblock replacement keeps '$' in the notes literal.
  $pattern = '(?s)## Unreleased\r?\n.*?(?=\r?\n## |\z)'
  if ($changelog -match $pattern) {
    $changelog = [regex]::Replace($changelog, $pattern, { param($m) $section })
  } else {
    $changelog = $changelog.TrimEnd() + "`n`n" + $section
  }
  $changelog | Set-Content CHANGELOG.md -Encoding utf8 -NoNewline
}

Write-Host "Cargo.toml : $($toml[$versionLine])"
Write-Host "Cargo.lock : $updated member entries set to $Version ($($members -join ', '))"
if ($NotesFile) { Write-Host "CHANGELOG  : added section $Version" }
exit 0
