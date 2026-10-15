<#
.SYNOPSIS
  Works out the next HyperStack release version from the commits since the last
  tag, and writes the release notes for them.

.DESCRIPTION
  Conventional Commit subjects are primary (feat:, fix:, perf:, security:,
  docs:, chore:, refactor:, test:, ci:, style:, build:). Plain-verb subjects are
  accepted as a fallback with the same meaning ("Add ..." = feat, "Fix ..." =
  fix, "Document ..." = docs), so a commit written without a type still
  releases correctly. A subject nobody can classify falls through to a patch
  bump on purpose: the failure mode is an extra patch release, never a release
  that silently never shipped.

  Bumps:  feat!: / BREAKING CHANGE: footer -> major
          feat:, Add/Introduce/Implement/Support/Enable/Redesign -> minor
          fix:, perf:, security:, Fix, any unknown verb -> patch
          docs:, chore:, refactor:, test:, ci:, style:, build:, Document, Merge -> nothing

  A "release: X.Y.Z" trailer in any commit body forces that version (used to
  force 1.0.0 at the end of Phase 1).

  Channels: "stable" tags vX.Y.Z from main; "beta" tags vX.Y.Z-beta.N, where N
  counts up from the previous beta of the same base version.

  Prints a summary and, under Actions, appends to $env:GITHUB_OUTPUT:
    version   the version to release, or empty when nothing warrants one
    tag       v<version>
    channel   stable | beta
    previous  the tag it is measured from, or empty on a first release
    republish true when the plan re-publishes an existing tag (see -Republish)

  Notes go to -NotesFile as Markdown (Added / Fixed / Changed / Security).
  Nothing else is modified, so this is safe to run locally at any time.

  -Republish <tag>: the workflow passes the channel's newest tag when the forge
  has no published release for it (a release job that tagged and pushed but
  died before publishing). When nothing new warrants a release, that version is
  planned again with republish=true and its notes read back out of
  CHANGELOG.md, so the next push to main publishes the tag instead of leaving
  it orphaned. A new release-worthy commit still wins and supersedes it.
#>
param(
  [ValidateSet("stable", "beta")][string]$Channel = "stable",
  # Used when no tag exists yet. Phase 0 exits with 0.1.0.
  [string]$FirstVersion = "0.1.0",
  [string]$NotesFile = "release/notes.md",
  # The channel's newest tag when it has no published release yet.
  [string]$Republish = ""
)

$ErrorActionPreference = "Stop"

$MinorVerbs     = @("Add", "Introduce", "Implement", "Support", "Enable", "Redesign")
$NoReleaseVerbs = @("Document", "Merge", "Revert")
$MinorTypes     = @("feat")
$PatchTypes     = @("fix", "perf", "security")
$NoReleaseTypes = @("docs", "chore", "refactor", "test", "ci", "style", "build")

function Set-Output([string]$name, [string]$value) {
  if ($env:GITHUB_OUTPUT) { "$name=$value" | Out-File $env:GITHUB_OUTPUT -Append -Encoding utf8 }
}

function Get-Tags([string]$pattern) {
  @(git tag --list "v*" --sort=-v:refname | Where-Object { $_ -match $pattern })
}

# The base for the next version is always the latest stable tag; the range of
# commits to describe starts at the latest tag of the channel being released.
# @() at every assignment: PowerShell unrolls a one-element array coming out
# of a function or an if-expression into a plain string, and "v0.1.0"[0] is
# "v" (release run 19 planned nothing after the first tag existed).
$stableTags  = @(Get-Tags '^v\d+\.\d+\.\d+$')
$channelTags = @(if ($Channel -eq "beta") { Get-Tags '^v\d+\.\d+\.\d+(-beta\.\d+)?$' } else { $stableTags })
$previous    = if ($channelTags.Count) { $channelTags[0] } else { "" }
# The bump is measured over everything since the last stable tag, so a beta
# after a feat beta still plans the minor (not a patch on the old stable);
# the notes describe only the commits since the channel's previous tag.
$bumpRange   = if ($stableTags.Count) { "$($stableTags[0])..HEAD" } else { "HEAD" }
$range       = if ($previous) { "$previous..HEAD" } else { "HEAD" }
$inNotes     = @{}
foreach ($h in @(git log --no-merges --format="%H" $range)) { $inNotes[$h] = $true }

# -z-style record separators: subjects can contain anything, including quotes.
$raw = git log --no-merges --format="%H%x1f%s%x1f%b%x1e" $bumpRange
$records = @(($raw -join "`n") -split "`u{1e}" | Where-Object { $_.Trim() -ne "" })

$bump = 0   # 0 none, 1 patch, 2 minor, 3 major
$forced = $null
$forcedHere = $false
$added = @(); $fixed = @(); $changed = @(); $security = @()

foreach ($record in $records) {
  $parts = $record.TrimStart("`r", "`n") -split "`u{1f}"
  $hash = $parts[0]; $subject = $parts[1]; $body = if ($parts.Count -gt 2) { $parts[2] } else { "" }
  if (-not $subject) { continue }

  if ($body -match '(?m)^release:\s*(\d+\.\d+\.\d+)\s*$') {
    $forced = $Matches[1]
    if ($inNotes[$hash]) { $forcedHere = $true }
  }

  $type = ""; $desc = $subject; $bang = $false
  if ($subject -match '^(?<type>[a-z]+)(\([^)]*\))?(?<bang>!)?:\s*(?<desc>.+)$') {
    $type = $Matches.type; $desc = $Matches.desc; $bang = [bool]$Matches.bang
  }
  $verb = if ($subject -match '^([A-Za-z][\w-]*)') { $Matches[1] } else { "" }
  $breaking = $bang -or ($body -match 'BREAKING[ -]CHANGE')
  $line = if ($inNotes[$hash]) { "- $($desc.Substring(0,1).ToUpper())$($desc.Substring(1)) ($($hash.Substring(0,8)))" } else { $null }

  if ($breaking) { $bump = [Math]::Max($bump, 3); $added += $line; continue }
  if ($type) {
    if ($NoReleaseTypes -contains $type) { continue }
    if ($MinorTypes -contains $type) { $bump = [Math]::Max($bump, 2); $added += $line; continue }
    if ($type -eq "security")        { $bump = [Math]::Max($bump, 1); $security += $line; continue }
    if ($type -eq "fix")             { $bump = [Math]::Max($bump, 1); $fixed += $line; continue }
    if ($PatchTypes -contains $type) { $bump = [Math]::Max($bump, 1); $changed += $line; continue }
    # Unknown type: treat like an unknown verb below.
  }
  if ($NoReleaseVerbs -contains $verb) { continue }
  if ($MinorVerbs -contains $verb)     { $bump = [Math]::Max($bump, 2); $added += $line; continue }
  if ($verb -eq "Fix")                 { $bump = [Math]::Max($bump, 1); $fixed += $line; continue }
  $bump = [Math]::Max($bump, 1); $changed += $line
}

$added    = @($added    | Where-Object { $_ })
$fixed    = @($fixed    | Where-Object { $_ })
$changed  = @($changed  | Where-Object { $_ })
$security = @($security | Where-Object { $_ })

# Something in the channel's own range must warrant the release: a beta with
# no new release-worthy commit since the previous beta plans nothing.
$worthy = ($added.Count + $fixed.Count + $changed.Count + $security.Count) -gt 0
if ((-not $worthy -and -not $forcedHere) -or ($bump -eq 0 -and -not $forced)) {
  if ($Republish -and $previous -and ($Republish -eq $previous)) {
    # The newest tag was pushed by a job that never published it: plan that
    # version again, building the current HEAD (which differs from the tag
    # only by commits that cut no release), with the notes the CHANGELOG
    # already carries for it.
    $version = $Republish -replace '^v', ''
    New-Item -ItemType Directory -Force (Split-Path $NotesFile -Parent) | Out-Null
    $section = Join-Path $PSScriptRoot "changelog-section.ps1"
    $written = $false
    if ((Test-Path $section) -and (Test-Path "CHANGELOG.md")) {
      try {
        & $section -Version $version -ChangelogFile "CHANGELOG.md" -OutFile $NotesFile
        $written = (Test-Path $NotesFile) -and -not [string]::IsNullOrWhiteSpace((Get-Content $NotesFile -Raw))
      } catch { $written = $false }
    }
    if (-not $written) { "### Changed`n`n- Release $version`n" | Set-Content $NotesFile -Encoding utf8 }
    Write-Host "Channel      : $Channel"
    Write-Host "Previous tag : $previous"
    Write-Host "Republish    : $Republish has no published release; planning it again"
    Write-Host "Next version : $version"
    Write-Host "Notes        : $NotesFile"
    Set-Output "version"   $version
    Set-Output "tag"       $Republish
    Set-Output "channel"   $Channel
    Set-Output "previous"  $previous
    Set-Output "republish" "true"
    exit 0
  }
  Write-Host "No commits since $(if ($previous) { $previous } else { 'the beginning' }) warrant a release."
  Set-Output "version" ""
  Set-Output "tag" ""
  Set-Output "channel" $Channel
  Set-Output "republish" "false"
  exit 0
}

if ($forced) {
  $base = $forced
} elseif (-not $stableTags.Count) {
  $base = $FirstVersion
} else {
  $n = ($stableTags[0] -replace '^v', '') -split '\.'
  [int]$major = $n[0]; [int]$minor = $n[1]; [int]$patch = $n[2]
  switch ($bump) {
    3 { $major++; $minor = 0; $patch = 0 }
    2 { $minor++; $patch = 0 }
    1 { $patch++ }
  }
  $base = "$major.$minor.$patch"
}

if ($Channel -eq "beta") {
  $betas = @(Get-Tags ("^v" + [regex]::Escape($base) + '-beta\.(\d+)$'))
  $n = 0
  foreach ($t in $betas) { if ($t -match '-beta\.(\d+)$') { $n = [Math]::Max($n, [int]$Matches[1]) } }
  $version = "$base-beta.$($n + 1)"
} else {
  $version = $base
}

$sections = @()
if ($added.Count)    { $sections += "### Added`n`n"    + ($added    -join "`n") }
if ($fixed.Count)    { $sections += "### Fixed`n`n"    + ($fixed    -join "`n") }
if ($changed.Count)  { $sections += "### Changed`n`n"  + ($changed  -join "`n") }
if ($security.Count) { $sections += "### Security`n`n" + ($security -join "`n") }
if (-not $sections.Count) { $sections += "### Changed`n`n- Release $version" }

New-Item -ItemType Directory -Force (Split-Path $NotesFile -Parent) | Out-Null
($sections -join "`n`n") + "`n" | Set-Content $NotesFile -Encoding utf8

$levels = @{ 0 = "forced"; 1 = "patch"; 2 = "minor"; 3 = "major" }
Write-Host "Channel      : $Channel"
Write-Host "Previous tag : $(if ($previous) { $previous } else { '(none)' })"
Write-Host "Bump         : $(if ($forced) { "forced to $forced" } else { $levels[$bump] })"
Write-Host "Next version : $version"
Write-Host "Notes        : $NotesFile"

Set-Output "version"   $version
Set-Output "tag"       "v$version"
Set-Output "channel"   $Channel
Set-Output "previous"  $previous
Set-Output "republish" "false"
exit 0
