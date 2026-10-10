# CI workflow drafts

These two files are the planned Forgejo Actions workflows described in
[../09-ci-cd-and-updates.md](../09-ci-cd-and-updates.md). They are kept here,
outside `.forgejo/workflows/`, until Phase 0 creates the Cargo workspace;
otherwise every push would start a job that fails on a missing `Cargo.toml`.

Move them with:

```bash
mkdir -p .forgejo/workflows && cp docs/ci/ci.yml docs/ci/release.yml .forgejo/workflows/
```

Both follow the constraints of the `git.zacx.net` runner: PowerShell steps
only, no published `uses:` actions, manual checkout with the job token in an
`http.extraheader`, `$ErrorActionPreference = "Stop"`, and explicit asset
verification before the job is allowed to go green.
