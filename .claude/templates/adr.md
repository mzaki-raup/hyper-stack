# Template — ADR (append to `docs/08-decisions.md`)

One record per decision that a later reader would otherwise have to guess at: a
new dependency, a new wire protocol, a storage format, a UI structure, a
platform restriction. Numbering continues from the last record in the file
(ADR-026 as of 2026-10-31). Status stays *Proposed* until the first code lands
against it.

---

```markdown
---

## ADR-0NN · <Short decision, in the file's voice>

**Context.** What forced the decision — the requirement, the constraint, the
thing that broke. One short paragraph.

**Options.** A · B · C — or a table where the trade-offs need columns:

| Option | For | Against |
|---|---|---|
| **<chosen>** | … | … |
| <rejected> | … | … |

**Decision.** The option, with the version or crate pinned where that matters
(`tiberius` with rustls; `rskafka` rather than `rdkafka`, which is C-bound).

**Consequences.** What this costs: a dependency behind a Cargo feature, a
longer CI build, a platform limit, a `cargo deny` exception, work this now
makes necessary elsewhere. Name the ADRs it interacts with.
```

## Before writing one

- Check `cargo deny` passes with the dependency (`deny.toml`), and prefer pure
  Rust and rustls.
- A heavy bundled build (DuckDB) goes behind a Cargo feature, off by default
  until its CI cost is measured — say so in Consequences.
- Reference the ADR number from the spec and from the code comment that
  implements it, the way the existing crates do.
