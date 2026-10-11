# 09 — CI/CD, releases and in-app updates

**Status:** draft · **Last updated:** 2026-10-10 · **Decision record:** [ADR-018](08-decisions.md)

## 1. Goals

- Every push is checked (format, lint, tests, build) before it can reach `main`.
- Every push to `main` that contains a user-visible change becomes a release
  automatically: version bumped, tag created, binaries built, release page
  published on the Forgejo instance with notes and verifiable assets.
- The app knows its own version, can check for a newer release, show the
  notes, download, verify and install it from **Settings → Updates**, and
  can roll back once. All of this must work for a portable copy on a USB
  stick and must never leave the user with a broken executable.
- No cloud services beyond the Forgejo instance at `git.zacx.net`.

## 2. Constraints of the Forgejo instance (learned from Eden Explorer)

| Constraint | Consequence for HyperStack |
|---|---|
| One self-hosted **Windows** runner | Windows builds first; Linux via cross-compilation on the same runner (Phase 2); macOS needs a Mac runner (Phase 3) |
| The runner cannot fetch published actions from `code.forgejo.org` (forgejo#2809) and has no Node | Workflows use **no `uses:` steps**; checkout is a few lines of `git` with the job token in an `http.extraheader`, never written to `.git/config` |
| The runner's working copy persists between jobs and is emptied on entry | `ci.yml` must not run on `main` while `release.yml` runs there; `main` is only reached through branches CI has already checked, and the release job re-runs the checks |
| The forge derives a release row from a pushed tag **asynchronously** | Create or fetch the release, upload assets, **then** PATCH the metadata, and confirm in a loop |
| A job can go green having uploaded nothing (PowerShell non-terminating errors) | `$ErrorActionPreference = "Stop"`, assert that every contracted asset exists locally **and** on the forge before the job ends |
| Tag author email decides the release's owner ("Ghost" otherwise) | Tagger identity is `HyperStack Release <owner@zacx.net>` |

## 3. Versioning

- **Semantic Versioning** `MAJOR.MINOR.PATCH`, optional pre-release
  `-beta.N`. Tags are `v1.4.0`, `v1.5.0-beta.1`.
- **Single source of truth**: `[workspace.package] version` in the root
  `Cargo.toml`; every crate uses `version.workspace = true`. The binary reads
  `CARGO_PKG_VERSION` and `build.rs` adds `HS_GIT_SHA`, `HS_BUILD_DATE`,
  `HS_CHANNEL`, `HS_TARGET` as compile-time env vars. The Windows executable
  also carries `FileVersion`/`ProductVersion` resources so Explorer's
  Properties dialog agrees with the app.
- **Shown** in: Settings → About (`1.4.0 (a1b2c3d · 2026-10-10 · stable · windows-x64)`),
  the title bar tooltip, the Launcher footer, `HyperStack.exe --version`,
  and the first line of every log file.
- **Automatic bump from commit subjects** (Conventional Commits) by
  `scripts/next-release.ps1`, run by the release workflow:

  | Commits since last tag contain | Bump |
  |---|---|
  | `BREAKING CHANGE:` footer or `!` after the type (`feat!:`) | major |
  | `feat:` | minor |
  | `fix:`, `perf:`, `security:` | patch |
  | only `docs:`, `chore:`, `refactor:`, `test:`, `ci:`, `style:`, `build:` | **no release** (job stops, nothing published) |

  Plain-verb subjects are accepted as a fallback with the same meaning, so a
  commit written without a type prefix still releases correctly:
  `Add …`/`Introduce …`/`Implement …`/`Support …`/`Enable …` = minor,
  `Fix …` = patch, `Document …`/`Merge …` = none, any other verb = patch.
  A `release: 2.0.0` trailer in any commit overrides the computed version.
  `[skip ci]` in the subject skips both workflows (used by the version-bump
  commit itself).
- **Channels**: `stable` (default) and `beta`. Pushes to `main` release to
  stable. Pushes to `beta` branch release pre-releases `vX.Y.Z-beta.N` with
  the forge's `prerelease` flag set. The app only offers releases from the
  channel chosen in Settings, and never offers a pre-release to a stable
  user.
- **One version authority.** The project is also published from a public
  GitHub mirror that shares this repository's history (§10). Only the
  Forgejo workflow computes, bumps, commits and tags a version; GitHub builds
  the tag it receives and takes the version from the tag name, so both
  forges publish the same version from the same commit by construction.

## 4. Workflows

The workflows live in `.forgejo/workflows/` and are already active. Both
detect a missing root `Cargo.toml` and finish green without building or
releasing anything, so documentation pushes are safe until Phase 0 creates the
workspace. The release tooling is four PowerShell scripts in `scripts/`
(`next-release.ps1`, `update-version.ps1`, `build-and-package.ps1`,
`verify-release.ps1`); they started from Eden Explorer's release scripts and
were rewritten for the workspace, the signed manifest and channels. Build and
release scripts are the one deliberate exception to the no-reuse rule in
ADR-014, because they are delivery tooling for the same forge, not product
code.

### 4.1 `ci.yml` — every branch except `main`, every pull request

```
checkout (manual) → rustup stable + rustfmt + clippy
→ cargo fmt --all --check            (hard gate: new codebase, formatted from day one)
→ cargo clippy --all-targets --locked -- -D warnings
→ cargo test --workspace --locked    (unit + egui_kittest snapshots; integration tests
                                      with containers run only when a `HS_INTEGRATION`
                                      secret is present)
→ cargo deny check                   (licences, advisories, duplicate crates)
→ cargo build --release --locked -p hyperstack
→ smoke: HyperStack.exe --version ; --self-test --data-dir <temp>
                                     (prints version, opens and closes a data folder, exit 0;
                                      the step fails if anything appeared under %APPDATA% or
                                      %LOCALAPPDATA%)
```

Before the workspace exists the job only checks out the commit, confirms that
every `scripts/*.ps1` parses, and passes.

Caching: `CARGO_HOME` and `target/` are kept on the runner between jobs
(the working copy is wiped, these live outside it) to keep a full build under
the runner's timeout.

### 4.2 `release.yml` — push to `main` (and `beta`), or manual dispatch

```
1  checkout main with full history and tags
2  scripts/next-release.ps1 -Channel stable|beta
                                     → version, tag, channel to $GITHUB_OUTPUT; release/notes.md
                                       (version empty = "nothing to release" or no Cargo.toml yet;
                                        every later step is skipped and the job is green)
3  same checks as ci.yml (fmt, clippy, test, deny)
4  scripts/update-version.ps1 -Version <v> -NotesFile release/notes.md
                                     → [workspace.package] version, every member in Cargo.lock,
                                       CHANGELOG.md section
   git commit "chore(release): v<v> [skip ci]" ; git tag -a v<v>
5  scripts/build-and-package.ps1 -Version <v> -Channel <c>
                                     → cargo build --release --locked -p hyperstack --target x86_64-pc-windows-msvc
                                       refuses if `HyperStack.exe --version` ≠ <v>
                                       release/dist/: exe, zip (+README.txt, LICENSE, notes),
                                       CHANGELOG-<v>.md, manifest.json (size + sha256 per asset),
                                       manifest.json.minisig (rsign2 with HS_MINISIGN_KEY, verified
                                       against keys/minisign.pub), SHA256SUMS.txt; asserts the contract
   (Phase 2: + x86_64-unknown-linux-gnu via cargo-zigbuild; aarch64 variants)
6  (optional) Authenticode sign HyperStack.exe when HS_SIGN_PFX secret exists
7  git push origin HEAD:main ; git push origin v<v>
8  publish: GET or POST /api/v1/repos/owner/hyper-stack/releases, upload every asset,
   PATCH name/body/prerelease last, then GET until the forge reports all assets and the body
9  scripts/verify-release.ps1 -Tag v<v>
                                     → anonymously downloads manifest + signature + every asset,
                                       checks signature, size, sha256, SHA256SUMS agreement,
                                       `--version` and `--self-test`; fails the job if anything is off
```

Nothing is pushed or published until step 5 succeeds, so a failed build leaves
no half-release behind. The release job refuses to run without the
`HS_MINISIGN_KEY` secret: an unsigned release would be rejected by every
installed copy anyway.

### 4.3 Asset contract (what the updater reads; renaming any of these is a breaking change)

| Asset | Purpose |
|---|---|
| `HyperStack.exe` | bare Windows x64 executable, downloaded by the in-app updater |
| `HyperStack-<version>-windows-x64.zip` | for manual updates: exe + `README.txt` + `LICENSE` |
| `manifest.json` | machine-readable release description (schema below) |
| `manifest.json.minisig` | Ed25519 signature of the manifest (minisign format) |
| `SHA256SUMS.txt` | `sha256sum`-style lines for every asset, for humans and scripts |
| `CHANGELOG-<version>.md` | the notes for this version, also used as the release body |
| Phase 2: `hyperstack-<version>-linux-x64.tar.gz`, `hyperstack` (bare) · Phase 3: `HyperStack-<version>-macos-universal.zip` | same scheme per platform |

`manifest.json`:

```json
{
  "schema": 1,
  "product": "HyperStack",
  "version": "1.4.0",
  "channel": "stable",
  "published_at": "2026-10-10T18:02:11Z",
  "tag": "v1.4.0",
  "notes_url": "https://git.zacx.net/owner/hyper-stack/releases/tag/v1.4.0",
  "min_supported_version": "1.0.0",
  "data_schema": 3,
  "assets": {
    "windows-x64": { "name": "HyperStack.exe", "url": "https://git.zacx.net/owner/hyper-stack/releases/download/v1.4.0/HyperStack.exe", "size": 31457280, "sha256": "…" },
    "windows-x64-zip": { "name": "HyperStack-1.4.0-windows-x64.zip", "url": "…", "size": 12582912, "sha256": "…" }
  }
}
```

`min_supported_version` lets a future release say "copies older than X must
update via the zip" (for example after an updater protocol change).
`data_schema` lets the app warn before installing a version whose data
format is newer than what the user's other copies understand.

### 4.4 Secrets on the forge

| Secret | Used for |
|---|---|
| `GITHUB_TOKEN` (provided by Forgejo) | checkout, push of the version commit and tag, release API |
| `HS_MINISIGN_KEY` | contents of the minisign secret key (generated with `rsign generate --unencrypted`, see `keys/README.md`; `-W` keys still prompt for a password and cannot be used in CI) that signs `manifest.json`; the matching public key is committed as `keys/minisign.pub`, compiled into the app and printed in About |
| `HS_SIGN_PFX`, `HS_SIGN_PFX_PASSWORD` (optional) | Authenticode code signing; without them the exe is unsigned and SmartScreen will warn on first run, which the README documents |
| `HS_INTEGRATION` (optional) | enables container-backed integration tests when a Docker engine is available on the runner |

The active key pair was generated on 2026-10-10; its public half is
`keys/minisign.pub` (key id `99C2A397C6491EAB`; an earlier `12CCB188A17A5843` key was retired unused). Key rotation: a new minisign
public key is shipped in a release signed by the old key; the app accepts any
key in its embedded list, and the list drops old keys two major versions later.

The GitHub mirror's release workflow needs the same `HS_MINISIGN_KEY` in its
own secret store (or a key of its own; §10.4).

## 5. In-app updater (`hs-core::update`, UI in Settings → Updates)

### 5.1 Settings page

```
┌ Updates ──────────────────────────────────────────────────────────────────────┐
│ Installed version   1.4.0  (a1b2c3d · built 2026-10-10 · stable · windows-x64) │
│ Data folder         D:\Tools\HyperStack\HyperStackData   (writable ✓)           │
│ Executable folder   D:\Tools\HyperStack                   (writable ✓)           │
│                                                                                 │
│ Channel             (●) Stable   ( ) Beta                                       │
│ Check for updates   [x] automatically, at most once a day, on startup           │
│ When found          (●) Tell me   ( ) Download, install when I exit             │
│                                                                                 │
│ Last checked 2026-10-10 17:58 · up to date          [ Check now ]               │
│                                                                                 │
│ ── Update available: 1.4.0 → 1.5.0 (published 2 h ago) ─────────────────────── │
│ ▾ What's new  (rendered Markdown from CHANGELOG-1.5.0.md)                       │
│   ● Added: Redis stream viewer …                                                │
│ [ Download (31 MB) ]  [ View on git.zacx.net ]  [ Skip this version ]           │
│ ▸ after download:  verified ✓ sha256 · signature ✓   [ Restart and install ]    │
│                                                                                 │
│ Previous version    1.3.2 (kept until next update)   [ Revert to 1.3.2 ]         │
│ Offline             [ Install from file… ]  (zip or exe + manifest + signature)  │
└─────────────────────────────────────────────────────────────────────────────────┘
```

A badge appears on the Settings icon and in the status bar when an update is
available; it is quiet (no toast) unless the release is marked `security`.

### 5.2 Check

1. At startup (if enabled, and the last check is older than 24 h) or on
   **Check now**: one anonymous HTTPS `GET` to
   `https://git.zacx.net/api/v1/repos/owner/hyper-stack/releases?limit=20`
   with `If-None-Match` from the cached ETag. Nothing is sent but the request;
   the user agent is `HyperStack/<version>`.
2. For each release: skip drafts, skip pre-releases unless channel is beta,
   parse the tag as SemVer, skip if not newer than the running version, skip
   if `manifest.json` or the platform's bare asset is missing (a release
   without a complete asset set is treated as not existing, never as "up to
   date" silently: the UI says "a newer release exists but has no installable
   assets yet").
3. Download the newest candidate's `manifest.json` and `.minisig`; verify the
   signature against the embedded public keys; refuse on mismatch with a red
   message naming the problem.
4. Compare `min_supported_version` (if the running copy is below it, offer the
   zip and a link instead of in-place update) and `data_schema` (warn if newer
   than what the running copy writes, since other copies of the data folder
   may not open it).
5. Show the result. Network failures are shown as "could not check" with the
   reason, never as "up to date".

### 5.3 Download and verify

- Download runs as a **Job** (progress, cancel, resume with `Range`) into
  `HyperStackData/update/<version>/HyperStack.exe.part`, written with
  `FILE_ATTRIBUTE_TEMPORARY` hints off and fsync at the end.
- Verify **size**, **sha256** from the signed manifest, and (when present)
  the Authenticode signature chain via `WinVerifyTrust`. Then run the file with
  `--self-test` in a child process (it must print its version, which must equal
  the manifest version, and exit 0 within 10 s). Only then is it renamed to
  `HyperStack.exe.ready`.
- Anything that fails deletes the partial file and reports which check failed.
  No downloaded file is ever executed before the sha256 and signature checks.

### 5.4 Install (Windows, portable)

A running executable cannot be overwritten but **can be renamed** on the same
volume, so no helper process or installer is needed:

```
1  refuse if: exe folder not writable · another HyperStack instance holds a lock on this
   data folder · a tool has unsaved work (prompt to save) · a transfer/job is running (prompt)
2  save layout and settings, flush history db, lock vault
3  rename  HyperStack.exe                      → HyperStackData\update\previous\HyperStack.exe   (keep one)
4  move    HyperStackData\update\1.5.0\HyperStack.exe.ready → HyperStack.exe
5  spawn   HyperStack.exe --finish-update --from 1.4.0 --waiting-for <this pid>   then exit(0)
6  new process: wait for the old pid to exit (≤ 30 s), write HyperStackData\update\state.toml
   {installed = "1.5.0", previous = "1.4.0", at = …, ok = false}, run data migrations, open the
   window, and only after the first successful frame set ok = true and show
   "Updated to 1.5.0 · What's new" once
```

Failure handling:

- If step 4 fails, step 3 is reverted (rename back) and the user sees why.
- If the new executable crashes before `ok = true`, the next start of **any**
  copy in that folder (the user double-clicks `HyperStack.exe` again) sees
  `ok = false` and offers "Revert to 1.4.0", which swaps `previous\HyperStack.exe`
  back. The crash log path is shown.
- `previous\` is kept until the next successful update; **Revert** in Settings
  performs the same swap in the other direction and is the only supported
  downgrade path (settings and data files written by the newer version stay
  readable where the schema allows, otherwise the older copy opens them
  read-only, see [05](05-data-model-and-storage.md) §5).
- If the executable's folder is read-only (Program Files, a read-only share),
  the Updates page explains it, offers the zip download and "open the release
  page", and the in-place steps are disabled.

Linux and macOS (Phase 2/3) use the same rename-and-relaunch scheme for a bare
binary in a writable directory; macOS app bundles replace the whole `.app`
after unzipping and re-sign check, and AppImage/package installs point users to
their package manager.

### 5.5 Offline and air-gapped machines

**Install from file…** accepts the release zip or the bare exe together with
`manifest.json` and `manifest.json.minisig` (all three can be dropped at once).
The same verification runs; the only difference is the source. Useful for
servers without internet and for IT departments that stage releases.

### 5.6 Policy and privacy

- Checking is on by default but clearly described and switchable; it is a
  single GET to the project's own forge, with no identifiers beyond the
  version in the user agent. There is no telemetry channel of any kind.
- Installation always requires a user action unless the user chose "install
  when I exit". Downgrades require the explicit Revert button.
- A `security` label on a release (set via the `security:` commit type)
  makes the notification a toast and highlights the entry.
- Enterprise override: `HyperStackData/settings.toml` can set
  `[updates] enabled = false` or `source = "https://mirror.internal/hyperstack/"`
  (a folder with the same asset layout); the manifest signature still has to
  verify, so a mirror cannot inject binaries.

## 6. Release notes and changelog

- `scripts/next-release.ps1` groups commits since the last tag into
  **Added** (`feat`), **Fixed** (`fix`), **Changed** (`perf`, `refactor` with
  a `user-facing:` trailer), **Security** (`security`), using the commit
  subject with the type prefix removed and the first letter capitalised, one
  bullet each, and links to the commit. Commits with a `notes:` trailer use
  that text instead.
- The section is prepended to `CHANGELOG.md` (committed with the version bump)
  and written to `CHANGELOG-<version>.md` as an asset and the release body.
- The app renders the notes with the Markdown preview (devtools), so links and
  code spans work.

## 7. Local developer workflow

```powershell
pwsh -File scripts/next-release.ps1                      # what the next push to main would release (writes release/notes.md only)
pwsh -File scripts/next-release.ps1 -Channel beta        # same for the beta branch
pwsh -File scripts/update-version.ps1 -Version 1.5.0     # local dry run of the bump (then `git checkout -- Cargo.toml Cargo.lock CHANGELOG.md`)
pwsh -File scripts/build-and-package.ps1 -Version 1.5.0 -SkipSign   # release/dist/ exactly like CI, unsigned (local only)
pwsh -File scripts/verify-release.ps1 -Tag v1.4.0        # checks a published release end to end
cargo run -p hyperstack -- --self-test                   # the smoke step CI runs
cargo run -p hyperstack -- --version
```

Rules for contributors: use Conventional Commit subjects; never edit the
version in `Cargo.toml` by hand; a release-worthy change that must not ship yet
goes in a branch, not on `main`. The scripts are the only place release logic
lives; if the workflow needs a new step, add it to a script first so it can be
run locally.

## 8. Phasing

| Phase | Deliverable |
|---|---|
| Now | `ci.yml`, `release.yml` and the four scripts are committed and active; both workflows pass without a Cargo workspace |
| Phase 0 | Cargo workspace with `[workspace.package] version`; `--version`, `--self-test`, `--data-dir`, version resources in the exe; `deny.toml`; minisign key pair generated, `keys/minisign.pub` committed, `HS_MINISIGN_KEY` secret set |
| Phase 0, exit | first push with a `feat:` commit publishes `v0.1.0` with the full asset contract and passes `verify-release.ps1` |
| M1 | Settings → Updates page with version display and **Check now**; download + verify + restart-and-install; revert; `--finish-update` |
| M6 (1.0) | Automatic daily check, badge, "install on exit", install from file, CHANGELOG rendering, Authenticode signing if a certificate is obtained |
| Phase 2 | Linux x64 builds via `cargo-zigbuild` on the Windows runner, beta channel, mirror source setting |
| Phase 3 | macOS runner and bundle, aarch64 builds, delta updates if size becomes a problem |

## 9. Tests

- Unit: SemVer parsing and ordering (`1.10.0 > 1.9.0`, pre-release ordering),
  release filtering (drafts, pre-releases, missing assets, wrong platform),
  manifest parsing and signature verification with a test key pair, sha256
  checks, `release-plan` bump rules over fixture commit lists, changelog
  rendering.
- Integration (Rust tests in `hs-core::update`): package a build into a temp
  dir and run the full install sequence against a dummy "old" executable,
  including the crash → revert path and the read-only-folder path.
- Scripts: `next-release.ps1` is exercised against fixture repositories
  (Pester 5 tests in `scripts/tests/next-release.Tests.ps1`, run by
  `ci.yml` and locally with `Invoke-Pester scripts/tests`) for every bump
  rule, the `release:` trailer, zero / exactly one / many tags, beta
  numbering and the no-release case. A beta's version is bumped over every
  commit since the last *stable* tag, while its notes cover only the commits
  since the previous beta; `ci.yml`
  parse-checks every script on each push.
- CI itself: `verify-release` after every publish is the end-to-end test of
  the pipeline; the job fails if the published release is not installable.

## 10. The public GitHub mirror

HyperStack is published from two places: this Forgejo instance and a public
repository on GitHub under a different account name. They are **one
repository with one git history**, not two code bases kept in step: a commit
is the same commit on both forges, and `v1.4.0` is the same sha on both.
Nothing is copied, filtered or rewritten between them. Copybara, `git
subtree` and similar were considered and rejected because the two trees
differ by three constants and two directories that cannot collide.

### 10.1 What differs, and where it is decided

| | Forgejo build | GitHub build |
|---|---|---|
| Update source the app polls | `https://git.zacx.net/api/v1/repos/owner/hyper-stack/releases` | `https://api.github.com/repos/<user>/hyper-stack/releases` |
| Project link in Settings → About | `git.zacx.net/owner/hyper-stack` | `github.com/<user>/hyper-stack` |
| Asset URLs in `manifest.json` and the zip's README | Forgejo release page | GitHub release page |
| Signing key | `keys/minisign.pub` | the same key by default; optionally one more via `HS_SIGNING_KEY_EXTRA` |

All of these are **build-time parameters, not code differences**.
`hs-core::update` reads `HS_RELEASES_URL`, `HS_FORGE_URL` and
`HS_SIGNING_KEY_EXTRA` with `option_env!` and falls back to the Forgejo
values when a variable is absent *or empty* (an unset workflow variable
arrives as an empty string), so a plain `cargo build` is a Forgejo build.
`hs-core/build.rs` declares `rerun-if-env-changed` for all three: cargo does
not track `option_env!` on its own, and a shared `CARGO_TARGET_DIR` would
otherwise let a GitHub build reuse a Forgejo-flavoured `hs-core`. A unit test
compares the baked-in constant with the run-time environment and fails on a
stale object file; `--self-test` refuses a source that is not `https://`.

`scripts/build-and-package.ps1 -RepoUrl` sets the asset base for the
manifest and the README. The GitHub pipeline must pass it because GitHub is
only a *push* URL on `origin` (§10.3), so the script's
`git remote get-url origin` fallback would answer with the Forgejo URL.
Forgejo cloned GitHub's releases API, so `ApiRelease`, the asset layout
`<repo>/releases/download/<tag>/<name>` and `scripts/verify-release.ps1`
work against both without a branch; only the API base differs
(`-Server https://github.com` selects `api.github.com`).

### 10.2 Workflows: two directories, one tree

`.forgejo/workflows/` and `.github/workflows/` sit side by side and each
forge ignores the other's directory, with one exception: Forgejo Actions
also reads `.github/workflows/`, and the GitHub workflows use published
actions the Forgejo runner cannot fetch (forgejo#2809). Every GitHub job
therefore carries `if: github.server_url == 'https://github.com'` and is
skipped, not failed, on Forgejo.

- `.github/workflows/ci.yml`: the same gate as `.forgejo/workflows/ci.yml`
  (fmt, clippy, tests, `cargo deny`, release build, `--self-test`, the
  portable check) on `main`, `beta` and pull requests, with the GitHub update
  source set so the tests exercise the configuration the release ships.
- `.github/workflows/release.yml`: runs on a `v*` tag only. It takes the
  version from the tag name, refuses if `Cargo.toml` at that commit says
  otherwise, re-runs the checks, reads the notes back out of `CHANGELOG.md`
  with `scripts/changelog-section.ps1` (identical text to the Forgejo
  release), builds with the GitHub source baked in, signs, publishes with
  `gh release create` and verifies the result anonymously with
  `verify-release.ps1`. It never bumps, commits or tags.

### 10.3 Version authority and the direction of sync

**Forgejo owns versioning.** Its `release.yml` computes the version, commits
the bump and tags (§3); that commit and tag reach GitHub through the shared
history and GitHub's release job fires on the tag. Both forges publish the
same version, from the same commit, with the same notes, by construction.

**Forgejo → GitHub** is an ordinary push. The owner's clone has GitHub as a
second push URL on `origin` and as a fetch remote of its own:

```bash
git remote set-url --add --push origin https://git.zacx.net/owner/hyper-stack.git
git remote set-url --add --push origin https://github.com/<user>/hyper-stack.git
git remote add github https://github.com/<user>/hyper-stack.git
```

so `git push origin main` lands on both, and `git push origin --tags` after
a Forgejo release hands GitHub the tag that starts its release job.

**GitHub → Forgejo** is deliberate, not automatic:
`pwsh -File scripts/sync-from-github.ps1` fetches, fast-forwards `main`
onto `github/main` and pushes to both. `--ff-only` is the safety: when the
two sides have diverged it stops and prints the graph instead of inventing a
merge.

Why neither direction is a mirror: a Forgejo *push mirror* force-pushes, so
a commit made on GitHub and not yet pulled would be silently discarded by
the next mirror run; a *pull mirror* makes the Forgejo repository read-only.
A plain push to a diverged remote is *rejected*, which is the behaviour
wanted when both sides can change. Automating both ways would also race the
release job's bump commit.

### 10.4 Secrets and identity on GitHub

| On GitHub | Kind | Value |
|---|---|---|
| `HS_MINISIGN_KEY` | Actions secret | the same two-line minisign secret key as on Forgejo (default), or a key of its own |
| `HS_SIGNING_KEY_EXTRA` | Actions **variable** | only when GitHub signs with its own key: that key's public base64 line, appended at build time to the keys the app accepts |
| `GITHUB_TOKEN` | provided | `gh release create` / `upload` |

Commit identity: the GitHub account name need not match the Forgejo one.
Only the author line in each commit is visible, and it is
`owner <owner@zacx.net>` on both forges; adding that address to the GitHub
account (Settings → Emails, verified) attributes the commits to the GitHub
user without rewriting anything. A *different author per forge* would
require rewriting history, which breaks the shared-history model; that is
the one requirement that would justify Copybara.
