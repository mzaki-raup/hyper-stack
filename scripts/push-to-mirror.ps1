<#
.SYNOPSIS
  Hands a Forgejo release to the GitHub mirror: pushes the version commit
  to the branch and the tag, so GitHub's release.yml builds the same version.

.DESCRIPTION
  Forgejo owns versioning (docs/09-ci-cd-and-updates.md section 10.3): its
  release job commits the bump and tags. GitHub builds a release only when
  the tag reaches it, so this runs as the last step of the Forgejo release
  job, after the release is published and verified.

  - Tag: always pushed. A failure here fails the step; the Forgejo release
    is already published, so a red job only means GitHub did not get it.
  - Branch: fast-forward only, never forced. If GitHub's branch has commits
    Forgejo lacks (someone pushed there), the branch push is skipped with a
    warning and the tag still goes out; reconcile with
    scripts/sync-from-github.ps1.
  - Not configured (no token or URL): prints a notice and exits 0, so a
    fork or a forge without the mirror still releases.

  - Release run: GitHub does not start push-triggered workflows for a
    commit whose message contains [skip ci], and that covers the tag push
    of Forgejo's "chore(release): vX.Y.Z [skip ci]". So when the tagged
    commit carries a skip marker, the script starts the mirror's
    release.yml through the REST API (workflow_dispatch with the tag);
    otherwise the tag push itself starts it and nothing is dispatched.

  The token goes on the git command line as an http.extraheader, never
  into .git/config. It needs write access to the repository's contents,
  to its workflows (a pushed history can touch .github/workflows/) and to
  Actions (to dispatch the release run): fine-grained PAT with Contents,
  Workflows and Actions read/write; classic: repo + workflow.

.EXAMPLE
  $env:HS_GITHUB_MIRROR_URL = "https://github.com/<user>/hyper-stack.git"
  $env:HS_GITHUB_MIRROR_TOKEN = "<token>"
  pwsh -File scripts/push-to-mirror.ps1 -Tag v0.12.1 -Branch main
#>
param(
  [Parameter(Mandatory)][string]$Tag,
  [string]$Branch = "main",
  [string]$MirrorUrl = $env:HS_GITHUB_MIRROR_URL,
  [string]$Token = $env:HS_GITHUB_MIRROR_TOKEN,
  # Pushes the tag's commit, not whatever HEAD is.
  [string]$Ref = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($MirrorUrl) -or ([string]::IsNullOrWhiteSpace($Token) -and $MirrorUrl -match '^https?://')) {
  Write-Host "GitHub mirror not configured (HS_GITHUB_MIRROR_URL / HS_GITHUB_MIRROR_TOKEN); skipping."
  exit 0
}

$commit = if ($Ref) { $Ref } else { $Tag }
$sha = (git rev-parse "$commit^{commit}" 2>$null)
if ($LASTEXITCODE -ne 0 -or -not $sha) { throw "Tag or ref '$commit' does not exist locally." }
$sha = $sha.Trim()

# Credential on the command line only. A file:// or local path mirror (the
# tests) needs none.
$auth = @()
if ($Token) {
  $basic = [Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes("x-access-token:$Token"))
  $auth = @('-c', "http.extraheader=Authorization: Basic $basic")
}

# Retries cover network hiccups only; a rejection is not retried.
function Invoke-Push([string]$what, [string]$refspec) {
  for ($attempt = 1; $attempt -le 3; $attempt++) {
    $out = & git @auth push --porcelain $MirrorUrl $refspec 2>&1
    $code = $LASTEXITCODE
    $text = ($out | Out-String)
    if ($code -eq 0) { return @{ ok = $true; text = $text } }
    if ($text -match 'rejected|non-fast-forward|fetch first|stale info') { return @{ ok = $false; text = $text } }
    if ($attempt -lt 3) {
      Write-Host "$what failed (attempt $attempt), retrying: $($text.Trim())"
      Start-Sleep -Seconds (2 * $attempt)
    } else {
      return @{ ok = $false; text = $text }
    }
  }
}

# The token never appears in output: git does not echo extra headers, and
# the URL carries no credentials.
$branchResult = Invoke-Push "Branch push" "${sha}:refs/heads/$Branch"
if ($branchResult.ok) {
  Write-Host "Mirror branch $Branch is at $($sha.Substring(0, 8))."
} else {
  Write-Warning ("The mirror's $Branch could not be fast-forwarded to $($sha.Substring(0, 8)) " +
    "(it has commits Forgejo lacks, or the push was refused). The tag is pushed anyway; " +
    "bring the branches together with scripts/sync-from-github.ps1.`n$($branchResult.text.Trim())")
}

$tagResult = Invoke-Push "Tag push" "refs/tags/${Tag}:refs/tags/$Tag"
if (-not $tagResult.ok) {
  throw "Pushing $Tag to the GitHub mirror failed; GitHub will not build this release.`n$($tagResult.text.Trim())"
}
Write-Host "Pushed $Tag to the GitHub mirror."

# Start the mirror's release run when the tag push will not (see above).
$message = (git log -1 --format=%B $sha) -join "`n"
if ($message -notmatch '\[(skip ci|ci skip|no ci|skip actions|actions skip)\]') {
  Write-Host "The tagged commit has no skip marker; the tag push starts GitHub's release workflow."
  exit 0
}
if ($MirrorUrl -notmatch '^https://github\.com/([^/]+)/([^/]+?)(\.git)?/?$') {
  Write-Host "The mirror is not a github.com repository; no release run to dispatch."
  exit 0
}
$api = "https://api.github.com/repos/$($Matches[1])/$($Matches[2])/actions/workflows/release.yml/dispatches"
$headers = @{
  Authorization          = "Bearer $Token"
  Accept                 = "application/vnd.github+json"
  "X-GitHub-Api-Version" = "2022-11-28"
}
$body = @{ ref = $Branch; inputs = @{ tag = $Tag } } | ConvertTo-Json -Depth 3
for ($attempt = 1; ; $attempt++) {
  try {
    Invoke-RestMethod -Method Post -Uri $api -Headers $headers -ContentType "application/json" -Body $body | Out-Null
    break
  } catch {
    if ($attempt -ge 3) {
      throw "Starting the GitHub release run for $Tag failed: $($_.Exception.Message). Run it by hand: Actions > Release > Run workflow, tag $Tag."
    }
    Write-Host "Dispatch failed (attempt $attempt), retrying: $($_.Exception.Message)"
    Start-Sleep -Seconds (3 * $attempt)
  }
}
Write-Host "Started GitHub's release workflow for $Tag (the [skip ci] commit does not start it on its own)."
exit 0
