# Release signing keys

`minisign.pub` is the public half of the Ed25519 key that signs every release's
`manifest.json`. It is committed here so `scripts/verify-release.ps1` can check
a release, and it is embedded in the app (`hs-core::update`) so the updater can
refuse anything not signed by it.

The private key exists only as the `HS_MINISIGN_KEY` secret on the forge and
in the owner's offline backup. It is never committed; `.gitignore` excludes
every file in this folder except `README.md` and `minisign.pub`, plus `*.key`
and `*.sec` anywhere in the tree.

## Current key

| Key id | Generated | Status |
|---|---|---|
| `99C2A397C6491EAB` | 2026-10-10 | active (signs every release from `v0.1.0`) |
| `12CCB188A17A5843` | 2026-10-10 | retired before any release: generated with `-W`, never used |

The key id is the one `rsign`/`minisign` print in the public key's comment
line and embed in every signature. Settings → About in the app shows the same
id so a user can compare it with this file.

Check a published release against it at any time:

```powershell
pwsh -File scripts/verify-release.ps1 -Tag v0.1.0
```

## Generating the pair (once, by the repository owner)

```bash
cargo install rsign2 --locked
rsign generate --unencrypted -p keys/minisign.pub -s hyperstack-minisign.key
```

**Use `--unencrypted`, not `-W`.** A key made with `-W` ("passwordless") is
still stored in the encrypted format and `rsign sign` asks for its (empty)
password on the console, which a CI job cannot answer; the release job detects
that and fails with a message pointing here. `--unencrypted` keys sign
non-interactively. Then:

1. Commit `keys/minisign.pub` and put the same base64 line into
   `SIGNING_KEYS` in `crates/hs-core/src/update.rs` (the app verifies with it).
2. Paste the **whole contents** of `hyperstack-minisign.key` (both lines: the
   `untrusted comment:` line and the base64 line) into the repository secret
   `HS_MINISIGN_KEY` on the forge (Settings → Actions → Secrets). The release
   script also accepts just the base64 line and CRLF endings.
3. Store `hyperstack-minisign.key` offline and delete it from the working
   machine.

## Rotation

Generate a new pair, add the new public key to `keys/` and to the app's
embedded key list (`SIGNING_KEYS` in `crates/hs-core/src/update.rs`), ship a
release signed with the **old** key, then switch `HS_MINISIGN_KEY` to the new
secret. The app accepts any embedded key; old keys are dropped two major
versions later (docs/09 §4.4).

## The GitHub mirror

The public GitHub repository publishes the same versions from the same
commits (docs/09 §10) and signs its manifests too. Two ways to set it up:

- **Same key (default, simplest).** Paste the same two-line secret into the
  GitHub Actions secret `HS_MINISIGN_KEY`. Nothing else changes; every binary
  already trusts `keys/minisign.pub`.
- **A key of its own.** Generate a second pair the same way, commit its public
  half as `keys/minisign-github.pub`, set the GitHub secret `HS_MINISIGN_KEY`
  to that secret key, and set the GitHub Actions **variable**
  `HS_SIGNING_KEY_EXTRA` to the public base64 line. The release build appends
  it to the app's accepted keys at build time, so GitHub-built copies trust
  both keys and Forgejo-built copies trust only the upstream one. Rotation of
  either key follows the section above.

Either way the private key file never enters the repository: `.gitignore`
covers both forges equally.
