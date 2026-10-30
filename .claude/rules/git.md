# Git rules

- Commit as the repository owner. Do not change the local git identity.
- **Conventional Commits**: `feat:`, `fix:`, `perf:`, `security:`, `docs:`,
  `chore:`, `refactor:`, `test:`, `ci:`, `style:`, `build:`. Add `!` or a
  `BREAKING CHANGE:` footer for a major bump.
- Never `--no-verify`, never `--no-gpg-sign`.
- Reference the ledger item in the footer, so history maps back to `TASKS.md`:

```
fix(tool-db): keep the schema cache across a reconnect

Refs: BUG-0003
```

- One commit per item in a `/dev-task` batch, on one branch
  (`batch/v0.7.1`, after the task version), so the batch gets one CI run and
  one merge.
- Gate commands must have their exit code checked, not their output eyeballed:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings >/dev/null 2>&1 && git commit
```

- Before committing a merge, a conflict-marker grep over `CHANGELOG.md` must
  print nothing:

```bash
grep -nE '^(<<<<<<<|=======|>>>>>>>)' CHANGELOG.md
```

## Branch flow (the owner asked for this every time)

1. Work on a branch.
2. Push it.
3. Wait for `ci.yml` to go green — check the forge's `/actions/tasks?limit=4`.
4. Merge into `main` and push.

Never merge while CI is red. The runner is this machine, so don't run local
cargo commands while a job is running.

## Remotes

`origin` is the Forgejo instance (`git.zacx.net`) and holds version authority;
pushing `origin` also pushes the public GitHub mirror. `github` is the mirror
alone. `scripts/push-to-mirror.ps1` and `scripts/sync-from-github.ps1` handle
the mirror; don't improvise around them.
