<#
.SYNOPSIS
  Brings commits made on the GitHub mirror back into Forgejo.

.DESCRIPTION
  The two forges share one git history (docs/09-ci-cd-and-updates.md section 10),
  so this is not a content sync: it is a fast-forward of the local branch onto
  whatever GitHub has, followed by a normal push.

  Forgejo -> GitHub needs no script: `git push origin main` goes to both
  because GitHub is configured as a second PUSH url on origin. This covers the
  other direction, which is deliberately manual - automating both ways buys a
  push loop and a race against the release job's version-bump commit.

  --ff-only is the point. If the two sides have genuinely diverged this stops
  and says so rather than inventing a merge commit; resolve it by hand, decide
  which side is right, and push.

.EXAMPLE
  pwsh -File scripts/sync-from-github.ps1
  pwsh -File scripts/sync-from-github.ps1 -Branch beta -NoPush
#>
param(
  [string]$Remote = "github",
  [string]$Branch = "main",
  # Fetch and fast-forward, but do not push back to origin.
  [switch]$NoPush
)

$ErrorActionPreference = "Stop"

function Invoke-Git([string]$what, [string[]]$gitArgs) {
  $out = & git @gitArgs 2>&1
  if ($LASTEXITCODE -ne 0) { throw "$what failed (git exit ${LASTEXITCODE}): $out" }
  return $out
}

if (-not (git remote | Where-Object { $_ -eq $Remote })) {
  throw "There is no '$Remote' remote. Add it with: git remote add $Remote https://github.com/<user>/hyper-stack.git"
}

$dirty = git status --porcelain
if ($dirty) { throw "The working tree is not clean; commit or stash first." }

$current = (git rev-parse --abbrev-ref HEAD).Trim()
if ($current -ne $Branch) { throw "On '$current' but syncing '$Branch'; switch branch first." }

Invoke-Git "fetch $Remote" @('fetch', '--quiet', '--tags', $Remote) | Out-Null
$before = (git rev-parse HEAD).Trim()
$theirs = (git rev-parse "$Remote/$Branch").Trim()

if ($before -eq $theirs) {
  Write-Host "Already in step with $Remote/$Branch ($($before.Substring(0, 7))); nothing to do."
  exit 0
}

# Is their tip a descendant of ours? If not, the two sides diverged.
& git merge-base --is-ancestor $before $theirs
if ($LASTEXITCODE -ne 0) {
  $ahead = (git rev-list --count "$theirs..$before").Trim()
  $behind = (git rev-list --count "$before..$theirs").Trim()
  throw @"
$Branch and $Remote/$Branch have diverged: $ahead local commit(s) they lack, $behind of theirs you lack.
A fast-forward is not possible. Inspect with:
  git log --oneline --graph --boundary $before...$theirs
then merge or rebase deliberately and push.
"@
}

Invoke-Git "fast-forward" @('merge', '--ff-only', "$Remote/$Branch") | Out-Null
Write-Host "Fast-forwarded $Branch to $($theirs.Substring(0, 7)):"
git log --oneline "$before..HEAD"

if ($NoPush) {
  Write-Host "`n-NoPush given; push with: git push origin $Branch"
  exit 0
}

# origin has both forges as push urls, so this re-publishes to Forgejo and is a
# no-op against GitHub.
Invoke-Git "push origin" @('push', 'origin', $Branch) | Out-Null
Write-Host "Pushed $Branch to origin (Forgejo + GitHub)."
exit 0
