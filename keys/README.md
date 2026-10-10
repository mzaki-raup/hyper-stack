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
| `12CCB188A17A5843` | 2026-10-10 | active (signs every release from `v0.1.0`) |

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
rsign generate -W -p keys/minisign.pub -s hyperstack-minisign.key
```

`-W` creates the secret key without a password so the release job can use it
non-interactively. Then:

1. Commit `keys/minisign.pub`.
2. Paste the **contents** of `hyperstack-minisign.key` into the repository
   secret `HS_MINISIGN_KEY` on the forge (Settings → Actions → Secrets).
3. Store `hyperstack-minisign.key` offline and delete it from the working
   machine.

## Rotation

Generate a new pair, add the new public key to `keys/` and to the app's
embedded key list, ship a release signed with the **old** key, then switch
`HS_MINISIGN_KEY` to the new secret. The app accepts any embedded key; old
keys are dropped two major versions later (docs/09 §4.4).
