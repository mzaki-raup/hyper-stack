<#
.SYNOPSIS
  Writes one released CHANGELOG.md section out to a notes file.

.DESCRIPTION
  The Forgejo pipeline generates release notes from the commit subjects
  (scripts/next-release.ps1 -> release/notes.md) and update-version.ps1 folds
  the same text into CHANGELOG.md before the release commit is tagged.

  The GitHub pipeline builds an existing tag rather than computing a release,
  so it cannot regenerate those notes - at the tag there are no commits since
  the tag. It reads them back out of CHANGELOG.md with this script instead,
  which is why both forges publish byte-identical notes for a version
  (docs/09-ci-cd-and-updates.md section 10).

.EXAMPLE
  pwsh -File scripts/changelog-section.ps1 -Version 0.9.0
#>
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [string]$ChangelogFile = "CHANGELOG.md",
  [string]$OutFile = "release/notes.md"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $ChangelogFile)) { throw "$ChangelogFile is missing." }
$text = Get-Content $ChangelogFile -Raw

# Sections are written by update-version.ps1 as "## <version> - <date>". Match
# the heading for this version (with or without a leading v and whatever
# follows on the line), then take everything up to the next "## " heading.
$esc = [regex]::Escape($Version)
$pattern = "(?s)^##\s+v?$esc(?:[^\r\n]*)?\r?\n(.*?)(?=\r?\n##\s|\z)"
$m = [regex]::Match($text, $pattern, [System.Text.RegularExpressions.RegexOptions]::Multiline)
if (-not $m.Success) {
  throw "$ChangelogFile has no '## $Version' section. The tag must be on the release commit that added it."
}

$notes = $m.Groups[1].Value.Trim()
if (-not $notes) { throw "The '## $Version' section in $ChangelogFile is empty." }

$dir = Split-Path -Parent $OutFile
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
($notes + "`n") | Set-Content $OutFile -Encoding utf8 -NoNewline

Write-Host "Wrote $(($notes -split "`n").Count) lines of notes for $Version to $OutFile"
exit 0
