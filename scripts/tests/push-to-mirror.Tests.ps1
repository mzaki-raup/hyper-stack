<#
  Pester 5 tests for scripts/push-to-mirror.ps1 against a local bare
  repository standing in for GitHub (no token needed for a path remote).

  Run:  Invoke-Pester scripts/tests
#>

BeforeAll {
  $script:Script = (Resolve-Path "$PSScriptRoot/../push-to-mirror.ps1").Path
  $script:Pwsh = (Get-Process -Id $PID).Path

  function New-TempDir {
    $d = Join-Path ([System.IO.Path]::GetTempPath()) ("hs-pm-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory $d | Out-Null
    $d
  }

  function Add-Commit([string]$Repo, [string]$Message) {
    Add-Content (Join-Path $Repo "f.txt") ([guid]::NewGuid().ToString())
    git -C $Repo add f.txt
    git -C $Repo -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false commit -q -m $Message
  }

  # Runs the script in $Repo; returns @{ code; text }.
  function Invoke-Mirror([string]$Repo, [string]$Url, [string]$Tag, [string]$Token = "") {
    Push-Location $Repo
    try {
      $env:HS_GITHUB_MIRROR_URL = $Url
      $env:HS_GITHUB_MIRROR_TOKEN = $Token
      $out = & $script:Pwsh -NoProfile -NonInteractive -File $script:Script -Tag $Tag -Branch main 2>&1
      @{ code = $LASTEXITCODE; text = ($out | Out-String) }
    } finally {
      Pop-Location
      Remove-Item Env:HS_GITHUB_MIRROR_URL, Env:HS_GITHUB_MIRROR_TOKEN -ErrorAction SilentlyContinue
    }
  }
}

Describe "push-to-mirror.ps1" {
  BeforeEach {
    $script:Root = New-TempDir
    $script:Src = Join-Path $Root "forgejo"
    $script:Mirror = Join-Path $Root "github.git"
    git init -q -b main $Src
    Add-Commit $Src "feat: first"
    git init -q --bare -b main $Mirror
    git -C $Src push -q $Mirror main
    Add-Commit $Src "chore(release): v0.2.0 [skip ci]"
    git -C $Src -c user.name=t -c user.email=t@example.invalid -c tag.gpgsign=false tag -a v0.2.0 -m v0.2.0
  }
  AfterEach { Remove-Item -Recurse -Force $script:Root -ErrorAction SilentlyContinue }

  It "skips quietly when the mirror is not configured" {
    $r = Invoke-Mirror $Src "" "v0.2.0"
    $r.code | Should -Be 0
    $r.text | Should -Match "not configured"
    git -C $Mirror tag --list | Should -BeNullOrEmpty
  }

  It "skips an https mirror without a token" {
    $r = Invoke-Mirror $Src "https://example.invalid/x.git" "v0.2.0"
    $r.code | Should -Be 0
    $r.text | Should -Match "not configured"
  }

  It "fast-forwards the branch and pushes the tag" {
    $r = Invoke-Mirror $Src $Mirror "v0.2.0"
    $r.code | Should -Be 0
    (git -C $Mirror rev-parse main) | Should -Be (git -C $Src rev-parse "v0.2.0^{commit}")
    git -C $Mirror tag --list | Should -Be "v0.2.0"
  }

  It "is a no-op the second time" {
    (Invoke-Mirror $Src $Mirror "v0.2.0").code | Should -Be 0
    (Invoke-Mirror $Src $Mirror "v0.2.0").code | Should -Be 0
  }

  It "still pushes the tag when the mirror's branch has diverged, and never forces" {
    $other = Join-Path $Root "github-clone"
    git clone -q $Mirror $other
    Add-Commit $other "docs: edited on GitHub"
    git -C $other push -q origin main
    $theirs = (git -C $Mirror rev-parse main)

    $r = Invoke-Mirror $Src $Mirror "v0.2.0"
    $r.code | Should -Be 0
    $r.text | Should -Match "could not be fast-forwarded"
    (git -C $Mirror rev-parse main) | Should -Be $theirs
    git -C $Mirror tag --list | Should -Be "v0.2.0"
  }

  It "fails when the tag does not exist" {
    $r = Invoke-Mirror $Src $Mirror "v9.9.9"
    $r.code | Should -Not -Be 0
    $r.text | Should -Match "does not exist"
  }
}
