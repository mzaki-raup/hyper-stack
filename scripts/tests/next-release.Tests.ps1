<#
  Pester 5 tests for scripts/next-release.ps1. Each test builds a throwaway git
  repository, makes commits and tags in it, runs the script there in a child
  pwsh (the script calls `exit`) and reads the planned version back from the
  GITHUB_OUTPUT file it appends to.

  Run:  Invoke-Pester scripts/tests
#>

BeforeAll {
  $script:Script = (Resolve-Path "$PSScriptRoot/../next-release.ps1").Path
  $script:Pwsh = (Get-Process -Id $PID).Path

  function New-FixtureRepo {
    $dir = Join-Path ([System.IO.Path]::GetTempPath()) ("hs-nr-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory $dir | Out-Null
    git -C $dir init -q -b main
    git -C $dir config user.name test
    git -C $dir config user.email test@example.invalid
    git -C $dir config commit.gpgsign false
    git -C $dir config tag.gpgsign false
    $dir
  }

  # One commit per message; a message may carry a body after a blank line.
  function Add-Commits([string]$Repo, [string[]]$Messages) {
    foreach ($m in $Messages) {
      $f = Join-Path $Repo "f.txt"
      Add-Content $f ([guid]::NewGuid().ToString())
      git -C $Repo add f.txt
      $msgFile = Join-Path $Repo ".git/MSG"
      Set-Content $msgFile $m -NoNewline
      git -C $Repo commit -q -F $msgFile
    }
  }

  function Add-Tag([string]$Repo, [string]$Tag) { git -C $Repo tag $Tag }

  # Runs the script and returns @{ version; tag; channel; previous; notes }.
  function Invoke-Plan([string]$Repo, [string]$Channel = "stable") {
    $out = Join-Path $Repo ".git/OUT"
    Remove-Item $out -ErrorAction SilentlyContinue
    $env:GITHUB_OUTPUT = $out
    Push-Location $Repo
    try {
      $null = & $script:Pwsh -NoProfile -NonInteractive -File $script:Script -Channel $Channel -NotesFile "$Repo/.git/notes.md" 2>&1
      if ($LASTEXITCODE -ne 0) { throw "next-release.ps1 exited ${LASTEXITCODE}" }
    } finally {
      Pop-Location
      Remove-Item Env:GITHUB_OUTPUT
    }
    $plan = @{ version = ""; tag = ""; channel = ""; previous = ""; notes = "" }
    foreach ($line in Get-Content $out) {
      $k, $v = $line -split '=', 2
      $plan[$k] = $v
    }
    if (Test-Path "$Repo/.git/notes.md") { $plan.notes = Get-Content "$Repo/.git/notes.md" -Raw }
    $plan
  }
}

Describe "next-release.ps1" {
  BeforeEach { $script:Repo = New-FixtureRepo }
  AfterEach { Remove-Item -Recurse -Force $script:Repo -ErrorAction SilentlyContinue }

  Context "first release (no tag yet)" {
    It "uses the first version for any release-worthy commit" {
      Add-Commits $Repo @("chore: scaffold", "feat: workspace")
      $p = Invoke-Plan $Repo
      $p.version | Should -Be "0.1.0"
      $p.tag | Should -Be "v0.1.0"
      $p.previous | Should -Be ""
    }

    It "plans nothing for documentation alone" {
      Add-Commits $Repo @("docs: vision", "Document the roadmap")
      (Invoke-Plan $Repo).version | Should -Be ""
    }
  }

  # Exactly one tag: the case that once unrolled into the string "v0.1.0"
  # (release run 19), so it is tested on its own.
  Context "bump rules after exactly one tag" {
    BeforeEach {
      Add-Commits $Repo @("feat: first")
      Add-Tag $Repo "v0.1.0"
    }

    It "<subject> -> <expected>" -ForEach @(
      @{ subject = "feat: terminal"; expected = "0.2.0" }
      @{ subject = "feat(editor): mermaid"; expected = "0.2.0" }
      @{ subject = "fix: crash"; expected = "0.1.1" }
      @{ subject = "perf: faster grid"; expected = "0.1.1" }
      @{ subject = "security: redact logs"; expected = "0.1.1" }
      @{ subject = "feat!: new layout"; expected = "1.0.0" }
      @{ subject = "Add a tool"; expected = "0.2.0" }
      @{ subject = "Implement S3"; expected = "0.2.0" }
      @{ subject = "Fix the updater"; expected = "0.1.1" }
      @{ subject = "Tweak something"; expected = "0.1.1" }
      @{ subject = "wip: unknown type"; expected = "0.1.1" }
    ) {
      Add-Commits $Repo @($subject)
      $p = Invoke-Plan $Repo
      $p.version | Should -Be $expected
      $p.previous | Should -Be "v0.1.0"
    }

    It "<subject> cuts no release" -ForEach @(
      @{ subject = "docs: readme" }
      @{ subject = "chore: deps" }
      @{ subject = "refactor: split" }
      @{ subject = "test: more" }
      @{ subject = "ci: cache" }
      @{ subject = "style: fmt" }
      @{ subject = "build: lockfile" }
      @{ subject = "Document the vault" }
      @{ subject = "Revert something" }
    ) {
      Add-Commits $Repo @($subject)
      (Invoke-Plan $Repo).version | Should -Be ""
    }

    It "treats a BREAKING CHANGE footer as major" {
      Add-Commits $Repo @("fix: rename asset`n`nBREAKING CHANGE: the zip is renamed")
      (Invoke-Plan $Repo).version | Should -Be "1.0.0"
    }

    It "takes the highest bump of all commits" {
      Add-Commits $Repo @("fix: a", "docs: b", "feat: c", "fix: d")
      (Invoke-Plan $Repo).version | Should -Be "0.2.0"
    }

    It "honours a release: trailer" {
      Add-Commits $Repo @("docs: phase 1 done`n`nrelease: 1.0.0")
      (Invoke-Plan $Repo).version | Should -Be "1.0.0"
    }

    It "ignores commits before the tag" {
      (Invoke-Plan $Repo).version | Should -Be ""
    }

    It "groups notes into sections" {
      Add-Commits $Repo @("feat: terminal search", "fix: tunnel leak", "security: vault", "perf: grid")
      $n = (Invoke-Plan $Repo).notes
      $n | Should -Match "### Added\s+- Terminal search"
      $n | Should -Match "### Fixed\s+- Tunnel leak"
      $n | Should -Match "### Security\s+- Vault"
      $n | Should -Match "### Changed\s+- Grid"
    }
  }

  Context "many tags" {
    It "measures from the newest stable tag by version order" {
      Add-Commits $Repo @("feat: a"); Add-Tag $Repo "v0.9.0"
      Add-Commits $Repo @("feat: b"); Add-Tag $Repo "v0.10.0"
      Add-Commits $Repo @("fix: c")
      $p = Invoke-Plan $Repo
      $p.previous | Should -Be "v0.10.0"
      $p.version | Should -Be "0.10.1"
    }
  }

  Context "beta channel" {
    BeforeEach {
      Add-Commits $Repo @("feat: first")
      Add-Tag $Repo "v0.1.0"
    }

    It "numbers the first beta 1" {
      Add-Commits $Repo @("feat: x")
      $p = Invoke-Plan $Repo "beta"
      $p.version | Should -Be "0.2.0-beta.1"
      $p.channel | Should -Be "beta"
    }

    It "counts up from the previous beta of the same base" {
      Add-Commits $Repo @("feat: x"); Add-Tag $Repo "v0.2.0-beta.1"
      Add-Commits $Repo @("fix: y")
      $p = Invoke-Plan $Repo "beta"
      $p.previous | Should -Be "v0.2.0-beta.1"
      $p.version | Should -Be "0.2.0-beta.2"
    }

    It "plans nothing when only docs follow the previous beta" {
      Add-Commits $Repo @("feat: x"); Add-Tag $Repo "v0.2.0-beta.1"
      Add-Commits $Repo @("docs: y")
      (Invoke-Plan $Repo "beta").version | Should -Be ""
    }

    It "describes only the commits since the previous beta" {
      Add-Commits $Repo @("feat: older"); Add-Tag $Repo "v0.2.0-beta.1"
      Add-Commits $Repo @("fix: newer")
      $n = (Invoke-Plan $Repo "beta").notes
      $n | Should -Match "Newer"
      $n | Should -Not -Match "Older"
    }

    It "is not seen by the stable channel" {
      Add-Commits $Repo @("feat: x"); Add-Tag $Repo "v0.2.0-beta.1"
      $p = Invoke-Plan $Repo
      $p.previous | Should -Be "v0.1.0"
      $p.version | Should -Be "0.2.0"
    }
  }
}
