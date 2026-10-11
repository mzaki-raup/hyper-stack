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

  The token goes on the git command line as an http.extraheader, never
  into .git/config. It needs write access to the repository's contents and,
  because a pushed history can touch .github/workflows/, to its workflows
  (fine-grained PAT: Contents + Workflows read/write; classic: repo +
  workflow). A tag pushed with a personal token starts GitHub workflows,
  which a push with GitHub's own GITHUB_TOKEN would not.

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
Write-Host "Pushed $Tag to the GitHub mirror; its release workflow builds it from there."
exit 0
