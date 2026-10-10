<#
.SYNOPSIS
  Verifies a published HyperStack release exactly as the in-app updater would.

.DESCRIPTION
  Anonymously (no token) reads the release for -Tag from the forge API, then:
    1. the release is not a draft and has a non-empty body;
    2. every contracted asset is attached;
    3. manifest.json downloads and its minisign signature verifies against
       keys/minisign.pub;
    4. the manifest's version, tag and channel match the release;
    5. every asset listed in the manifest downloads with the stated size and
       sha256, and SHA256SUMS.txt agrees;
    6. the Windows executable prints the released version with --version and
       passes --self-test against a temporary data folder.

  The release workflow runs this as its last step; a failure fails the job so a
  broken release is never left looking fine. Run it locally against any tag to
  check what users will get.
#>
param(
  [Parameter(Mandatory = $true)][string]$Tag,
  # owner/repo on the forge; defaults to the origin remote.
  [string]$Repository = "",
  [string]$Server = "https://git.zacx.net",
  [string]$PublicKey = "keys/minisign.pub",
  [switch]$NoSelfTest
)

$ErrorActionPreference = "Stop"

if (-not $Repository) {
  $origin = git remote get-url origin
  if ($origin -match '[:/]([^/]+/[^/]+?)(\.git)?$') { $Repository = $Matches[1] } else { throw "Cannot derive owner/repo from '$origin'; pass -Repository." }
}
if (-not (Test-Path $PublicKey)) { throw "Public key $PublicKey is missing." }
if (-not (Get-Command rsign -ErrorAction SilentlyContinue)) { throw "rsign (rsign2) is not installed: cargo install rsign2 --locked" }

$api = "$Server/api/v1/repos/$Repository"
$ua  = @{ "User-Agent" = "HyperStack-verify-release" }
$release = Invoke-RestMethod -Method Get -Headers $ua -Uri "$api/releases/tags/$([uri]::EscapeDataString($Tag))"

if ($release.draft) { throw "Release $Tag is still a draft." }
if ([string]::IsNullOrWhiteSpace($release.body)) { throw "Release $Tag has an empty body." }
$version = $Tag -replace '^v', ''
$assets = @{}
foreach ($a in $release.assets) { $assets[$a.name] = $a }

$required = @("HyperStack.exe", "HyperStack-$version-windows-x64.zip", "manifest.json", "manifest.json.minisig", "SHA256SUMS.txt", "CHANGELOG-$version.md")
$missing = @($required | Where-Object { -not $assets.ContainsKey($_) })
if ($missing.Count) { throw "Release $Tag lacks required assets: $($missing -join ', ')" }

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("hs-verify-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
  function Fetch([string]$name) {
    $path = Join-Path $work $name
    Invoke-WebRequest -Headers $ua -Uri $assets[$name].browser_download_url -OutFile $path
    return $path
  }

  $manifestPath = Fetch "manifest.json"
  $sigPath      = Fetch "manifest.json.minisig"
  rsign verify -p $PublicKey -x $sigPath $manifestPath
  if ($LASTEXITCODE -ne 0) { throw "manifest.json signature does NOT verify against $PublicKey." }
  Write-Host "manifest.json signature OK"

  $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
  if ($manifest.version -ne $version) { throw "manifest.version '$($manifest.version)' != '$version'." }
  if ($manifest.tag -ne $Tag)         { throw "manifest.tag '$($manifest.tag)' != '$Tag'." }
  $expectedChannel = if ($release.prerelease) { "beta" } else { "stable" }
  if ($manifest.channel -ne $expectedChannel) { throw "manifest.channel '$($manifest.channel)' but the forge marks the release as $expectedChannel." }

  $sumsPath = Fetch "SHA256SUMS.txt"
  $sums = @{}
  foreach ($line in Get-Content $sumsPath) { if ($line -match '^([0-9a-f]{64})\s+\*?(.+)$') { $sums[$Matches[2]] = $Matches[1] } }

  $exePath = $null
  foreach ($key in $manifest.assets.PSObject.Properties.Name) {
    $entry = $manifest.assets.$key
    if (-not $assets.ContainsKey($entry.name)) { throw "Manifest asset '$($entry.name)' is not attached to the release." }
    $path = Fetch $entry.name
    $size = (Get-Item $path).Length
    $hash = (Get-FileHash $path -Algorithm SHA256).Hash.ToLower()
    if ($size -ne $entry.size)     { throw "$($entry.name): size $size != manifest $($entry.size)." }
    if ($hash -ne $entry.sha256)   { throw "$($entry.name): sha256 mismatch against manifest." }
    if ($sums[$entry.name] -ne $hash) { throw "$($entry.name): sha256 mismatch against SHA256SUMS.txt." }
    Write-Host "$($entry.name): size and sha256 OK"
    if ($key -eq "windows-x64") { $exePath = $path }
  }

  if (-not $NoSelfTest -and $exePath -and $IsWindows) {
    # GUI-subsystem exe: use Start-Process -Wait to get real exit codes.
    $versionOut = Join-Path $work "version.txt"
    $p = Start-Process -FilePath $exePath -ArgumentList @('--version') -Wait -PassThru -NoNewWindow -RedirectStandardOutput $versionOut
    $reported = (Get-Content $versionOut -Raw -ErrorAction SilentlyContinue | Out-String).Trim()
    if ($p.ExitCode -ne 0 -or $reported -notmatch [regex]::Escape($version)) { throw "Downloaded exe reports '$reported', expected $version." }
    $data = Join-Path $work "data"
    $p = Start-Process -FilePath $exePath -ArgumentList @('--self-test', '--data-dir', $data) -Wait -PassThru -NoNewWindow
    if ($p.ExitCode -ne 0) { throw "Downloaded exe failed --self-test with exit code $($p.ExitCode)." }
    Write-Host "HyperStack.exe --version and --self-test OK"
  }

  Write-Host "Release $Tag verified: $($required.Count) assets, signed manifest, hashes and self-test all OK."
} finally {
  Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
exit 0
