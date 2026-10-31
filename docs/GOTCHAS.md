# GOTCHAS.md

Known mistakes and their fixes. Read the section for your area before starting work, and add new entries under the matching heading as `- [YYYY-MM-DD] Symptom → Correct approach`.

## Git, Merges and Releases

- **A merge resolution and its commit must be one `&&` chain, and the
  marker count a hard gate inside it.** On 2026-10-30 the resolver
  script died on a `/tmp` path, the `;` after it let `git add && git
  commit && git push` run anyway, and a merge with `<<<<<<<` in
  `CHANGELOG.md` reached `main` (run 2346, cancelled by the repair push
  a minute later). The shape that cannot do this is
  `resolve && test "$(grep -c '^<<<<<<<\|^=======\|^>>>>>>>' CHANGELOG.md)" = 0 && git commit …`
  - every step a gate, nothing after a `;`.

- **Never read a push's result through `| grep -v remote | tail -1`.**
  The Phase 3 merge was "pushed" that way on 2026-10-29, the push
  failed, the pipeline printed nothing, and `main` sat a release behind
  until the next branch was cut from `origin/main` and conflicted with
  work that assumed the merge was there. A push is a gate: check its
  exit code (`git push … || echo PUSH FAILED`) and `git log -1
  origin/main` after it, exactly as a `cargo` gate is checked by its
  exit code and not by eye.

- **Commit gates must check exit codes.** `cargo clippy ... | grep ... ; git commit` committed code with lint errors. Use `cargo clippy ... >/dev/null 2>&1 && git commit`, not a check on printed output.
- **Never use `--no-verify` or `--no-gpg-sign`.** If GPG signing fails, fix the agent (see the gpg-agent note below). Don't bypass it.
- **gpg-agent cache.** `%APPDATA%\gnupg\gpg-agent.conf` sets `default-cache-ttl 14400`. The agent re-reads it only after `gpgconf --reload gpg-agent`. `gpgconf --kill gpg-agent` drops the cache.
- **CHANGELOG conflict markers.** A merge of `vX.Y.Z` into a feature branch left `<<<<<<<`, `=======`, and `>>>>>>>` in `CHANGELOG.md` for five releases, and nothing caught it. Run `grep -n '^<<<<<<<\|^=======\|^>>>>>>>' CHANGELOG.md` before any merge commit. CI now fails on markers in tracked text.
- **Resolving a CHANGELOG conflict.** Never keep "both". The released section belongs to the release workflow, so keep its generated text as is. Unreleased keeps only items not yet released. Confirm with `grep -c '^<<<<<<<\|^=======\|^>>>>>>>' CHANGELOG.md` (must print `0`).
- **Writing Unreleased notes before a release lands** guarantees that conflict. Expect it, and resolve it as above.
- **New workspace member on a branch + merged release bump.** `update-version.ps1` can't see a crate that exists only on the branch, so `Cargo.lock` keeps its old version and every `--locked` command fails with "cannot update the lock file". Run a plain `cargo check`, then commit the one-line lock fix. The release job may already have tagged `main`.
- **Diff base.** `git diff <base>..HEAD` for "the last N days" needs the right base commit. A wrong base produced empty diffs for six crates in review.
- **Fluent/English locale** is the source for all visible strings. Edit `locales/en.ftl` with the Edit tool, never a heredoc. (See Patch Scripts.)

## Release Pipeline and Updater

- **CI must pass `-j 4` like everything else on this machine.** The Forgejo runner *is* the owner's Windows box, so an unbounded `cargo test --workspace` there exhausts the page file exactly as it does from a shell: run 471 died with `rustc-LLVM ERROR: out of memory` / `STATUS_ILLEGAL_INSTRUCTION` compiling `hs-container`'s lib test, on a commit that changed only Markdown and PNGs. `.claude/rules/build-and-run.md` has mandated `-j 4` for local work all along; `.forgejo/workflows/{ci,release}.yml` did not, and now do. The `.github/` copies are deliberately left unbounded - those run on GitHub-hosted machines. `scripts/build-and-package.ps1` still builds unbounded. Fixed 2026-10-31.
- **An out-of-memory CI failure looks like a code failure.** It is reported as `could not compile <crate>`, and the crate named is simply whichever one was unlucky, not the one at fault. Before debugging the code, check whether anything else was running on the machine - a GUI app, a screenshot harness, another cargo - and re-run on a quiet machine.

- **Asset names are a contract** with `hs-core::update`. Don't rename `HyperStack.exe`, `HyperStack-<version>-windows-x64.zip`, `manifest.json`, `manifest.json.minisig`, `SHA256SUMS.txt`, or `CHANGELOG-<version>.md`.
- **Release job OOM (run 79)** happened while the runner was busy. Keep release builds separate from other heavy jobs.
- **`wry::WebView::open_devtools`** exists only with `debug_assertions` or the `devtools` feature. Debug builds, clippy, and tests pass without it. Only the CI release build fails (run 61). The binary enables `features = ["devtools"]`. Check with `cargo build --release --locked -p hyperstack` before pushing.
- **Renaming a running exe** is allowed on the same volume. Overwriting it is not. The updater renames old → moves new → relaunches, so keep `HyperStackData/update/` on the same volume as the exe.
- **Updater downloads the zip.** The bare exe is over the forge edge's 100 MB limit, so the updater downloads the release zip and extracts the exe from it.
- **Runner `%TEMP%` is upper case.** The runner service uses `C:\WINDOWS\SystemTemp` (LocalSystem), but `canonicalize` returns `C:\Windows\…`. Build expected paths from the canonical folder, or compare case-insensitively (see `crates/hyperstack/src/editor_tests.rs`).
- **Tag and mirror.** The release job's last step fast-forwards GitHub to the version commit (never forced), pushes the tag, and dispatches GitHub's workflow. Never set a Forgejo push mirror while GitHub is writable.
- **Changes made on GitHub** come back with `scripts/sync-from-github.ps1` (fast-forward only).
- **Every GitHub job** is guarded with `if: github.server_url == 'https://github.com'`. Forgejo Actions also reads `.github/workflows/` and can't fetch published actions.
- **Release notes** are generated from commit subjects. Hand-edit only the Unreleased section.

## Patch Scripts and Text Editing

- **Re-running a half-applied patch duplicates work** (two `LspStatus` definitions, two toolbar blocks). Make each step idempotent: test for a marker that exists only after that step. Don't test the first line of the replacement, since it usually appears in the anchor too. Write each step to disk as it succeeds.
- **`grep -c` the markers after any re-run.** Checking "new text present" isn't enough when the new text starts with the old text.
- **Unicode escapes in Python.** `\u{b7}` or `\u{2192}` in a Python string is an error ("truncated \uXXXX escape"). Put the character itself in the text, or build it with `chr()`. Doubling the backslash writes a literal `\\u` into Rust, which compiles to the wrong thing.
- **Windows paths in Python.** Use raw strings (`r"C:\Users\..."`). `"C:\Users"` fails on `\U`.
- **Windows paths in TOML.** Use single-quoted literal strings (`kubeconfig_path = 'C:\Users\...'`). A double-quoted TOML string processes `\U`, and an invalid escape fails the whole file. The symptom was only "skipping connection …: TOML parse error" and a profile that never appeared.
- **Heredocs.** Use `<<'EOF'` (quoted). An unquoted heredoc expands `` `backticks` `` (blanking names in doc comments) and `$name` (turning Fluent text into `Remove { }?`, a parse error that dropped the whole message). Better still, write the script to a file with the Write tool and run it.
- **`the_bundled_locale_parses_without_junk`** fails the gate on bad Fluent entries. A locale error otherwise shows only as "locale en has 1 syntax errors" in the log.
- **Quoting accidents.** Two lost patches came from quoting, not logic. Write scripts to files.
- **Python on Windows writes CRLF.** `open(p, 'w')` translates `\n` to `\r\n`, so a patched LF file comes back with every line changed (git warns "CRLF will be replaced by LF"). Open with `newline=''` for reading and writing. Fix a damaged file with `sed -i 's/\r$//'`.
- **The Bash tool can refuse a long heredoc** ("unexpected EOF while looking for matching `'`") even when quoted, depending on the text. Nothing in it ran. Write the script to a file with the Write tool and run the file.

## Windows Environment and Build

- **Disk filled by `target/debug`.** It reached 280 GB (150 GB in `debug/incremental`) and filled D:, causing `os error 112` in the middle of `cargo test --workspace`. Delete `target/debug/incremental` (cache only; `debug/deps` stays, and the next build isn't a full rebuild). Check after long sessions.
- **`LNK1140: limit exceeded for program database`.** The PDB outgrew its format limit. No code change causes or fixes it. Don't set `CARGO_PROFILE_DEV_DEBUG=0` on the command line. That changes every fingerprint, rebuilds the whole workspace, and fills D: quickly. If it recurs, set the debug level in `Cargo.toml` so the fingerprint is stable.
- **Paging file exhaustion.** `cargo test --workspace` can fail with "the paging file is too small" (os error 1455), then "found invalid metadata files for crate …" on every later run. The parallel build exhausts the commit limit, not RAM. Delete `target/debug` and use `-j 4`. A rustc killed mid-write leaves corrupt `.rmeta`/`.rlib` files.
- **Two cargo invocations on one target folder** serialise on the build lock.
- **A background `cargo test` started before a new crate is registered** fails on the missing module. Re-run after registering.
- **DSH Windows sandbox** can fail with `SetNamedSecurityInfoW … grantWrite` or `sandbox-local windows-acl temp grant materialization failed`. Don't widen the workspace or edit ACLs by hand. Run the `diagnose-windows-sandbox-acl` skill on the failing path and on `%TEMP%`.
- **Resolving a command on PATH.** On Windows only, resolve through PATHEXT. Never fall back to the bare name, since a file with no extension can't be started ("%1 is not a valid Win32 application"). `jdtls` shipped both `bin/jdtls` (Python) and `bin/jdtls.bat`, and the bare path won.
- **Refreshing PATH.** A process keeps the PATH it started with. `hs_core::env_path::refresh` re-reads `HKCU\Environment` and the machine's `Session Manager\Environment` (expanding `REG_EXPAND_SZ`). Settings → Editor → Refresh Status calls it. Never `std::env::set_var("PATH", ...)`: it's unsound while other threads read the environment. Keep the value beside it, and have lookups ask for it.
- **Canonical paths.** `std::fs::canonicalize` adds a `\\?\` prefix. Strip it before storing or comparing editor project roots, or saved paths and URIs stop matching.
- **Child windows.** Windows Explorer or other installs made while HyperStack is open are invisible until restart. The same applies to tools installed while a shell was open.
- **No console windows.** An internal command with a bare `std::process::Command::new` flashes a console. Use `hs_core::paths::background_command` (sets `CREATE_NO_WINDOW`). The microphone, Read Aloud, and the updater's `tasklist` probe all did this until 2026-10-24.
- **Deliberate OS hand-offs** (`explorer`, `open`, `xdg-open` for Reveal in Explorer, and the updater relaunching the app) are the only commands allowed to open things outside the app.
- **Clipboard files (CF_HDROP).** A Windows file list is a 20-byte `DROPFILES` header followed by UTF-16 paths, each NUL-terminated, with an extra NUL ending the list. It carries paths, not bytes. "Copy a remote file" must download to `HyperStackData/tmp/` first (cleared at start). See `hs_core::clipboard_files`.
- **Global shortcuts.** Ctrl+Shift+L is `vault.lock`. The editor uses Alt+F3 for select-all-occurrences. Pick a different combo when there's a collision.
- **Release build fails with "Access is denied".** A running `HyperStack.exe` keeps its own file locked. Run `Stop-Process -Name HyperStack -Force` before `cargo build --release`. During development, always use `--data-dir ./tmp-data` so the real `HyperStackData/` is never touched.

## Build and Dependency Gotchas (Rust)

- **Edition 2024 reserved keyword.** `gen` fails to parse ("expected identifier, found reserved keyword"), which broke 18 places from one field. Name it `generate`.
- **`egui::Id::new(x)`** needs `x: Hash + Debug`. A helper taking `id_salt: impl Hash` must add `+ std::fmt::Debug`, or use `Id::new(base).with(salt)`.
- **Items after a test module.** Appending functions to a file puts them after `#[cfg(test)] mod tests`, which clippy refuses ("items after a test module"). Insert before the test module. `cat >>` is safe only for files without one.
- **`serde_json::Value::Object` is a sorted map.** Field order is lost. The JSON tree can't keep order without `preserve_order`, which would change every tool's JSON. For JWTs, re-serialising changes the signature input. Compact the user's text instead (`jwt::compact_json`).
- **rust-embed.** `#[include]` and `#[exclude]` need the `include-exclude` feature. Without it the derive fails silently, and every `Locales::get` call reports "no associated function".
- **`kafka-protocol` implements `Request` only with both `client` and `broker` on.** With `client` alone the messages encode, but `KEY`, the response type and `send::<R: Request>` do not exist ("the trait `Request` is not implemented for `ApiVersionsRequest`"). Neither feature adds a dependency; the compression features do (lz4 and zstd are C).
- **`async-nats` consumer `Info::paused` is behind a server-version feature** (`server_2_11`); with only `server_2_10` on the field does not exist.
- **bson extended JSON.** `Bson::try_from(serde_json::Value)` and `into_relaxed_extjson` need bson's `serde_json-1` feature, which the driver doesn't enable. Depend on `bson` directly with that feature.
- **mysql_async + rustls.** `ClientConfig::builder()` panics when two crypto providers are compiled in (reqwest brings aws-lc-rs, the app uses ring). Install ring as the process default once before connecting (`driver::mysql::install_crypto_provider`).
- **tokio-rustls, the same panic.** `Network Check`'s TLS pane hit it too:
  the handshake runs on the tokio runtime, so the panic killed the task
  silently and the spinner never stopped. `tool-devtools::net::install_crypto_provider`
  installs ring once, and the handshake itself carries a 10 s timeout so a
  stuck peer can't hang the pane either (2026-10-31).
- **reqwest.** The workspace reqwest has no `form` or `multipart` feature. `send::form_body` and `send::multipart` build the bodies by hand.
- **russh-sftp `write()`** opens with `WRITE` only and fails on a file that doesn't exist. Use `create()` (`CREATE | TRUNCATE | WRITE`).
- **Boa (JavaScript).** Native closures must be `Copy`. Host state goes through a `thread_local!` record (`script::HOST`), written before `run` and read after. `JsValue::to_json` needs `&mut Context`. Strings come out as `to_std_string_escaped()`.
- **`Tool` is `Send`.** A per-frame cache on tool state must be `Arc`, not `Rc`.
- **`wry` is a Windows-only dependency** of the binary (`[target.'cfg(windows)'.dependencies]`). The tool crate must stay free of platform code so Linux clippy passes.

## Clippy and Lints

- **Don't let lints pile up.** Fix them, or use a scoped `#[allow(...)]` with a reason. Never a crate-wide allow.
- **Clippy must pass with `-D warnings`** on every crate, including Linux.

## Shell and UI Layout (egui)

- **`split_for_assistant` hands back a child `Ui` whose clip rectangle
  *excludes* the assistant's column.** Drawing the column through that
  child (`child.new_child(...)` with the panel rect) clips every pixel
  of it away: the toggle lit up, the content narrowed, and the column
  stayed empty (Git activity, 2026-10-30, two rebuilds to see it). Keep
  the outer `Ui` (`let outer = ui; let ui = &mut content;`, as the
  terminal does) and make the column's child from `outer`.

- **egui's file loader does not take a bare path, and a `file://` URI
  with a `?v=` query is not found either.** Both drew the "could not
  load" warning triangle in place of the uploaded repository icon
  (2026-10-29, twice on screen). What works is what the API tool's home
  icon does: read the bytes and `ctx.include_bytes("bytes://…", bytes)`
  once per (path, modification stamp) - `hs_ui::icons::image_uri` keeps
  the set of names it has registered so an unchanged file costs a `stat`
  per frame and nothing more.

- **A `right_to_left` scope beside other widgets on one `horizontal`
  row gets only the width that is left**, and in a narrow column that
  is less than its button needs: the commit box of the Git activity
  drew "Push After Commit" and "Commit 2 file(s)" on top of each other
  (2026-10-29, seen on screen). Put the right-aligned control on a row
  of its own, or measure before deciding they fit.
- **Switching workspaces must park the live `TabSpace`, not rebuild
  it.** Dropping the space drops every tool: the browser's session, an
  SSH shell, a database connection - and "switch back" rebuilt fresh
  copies from the saved layout that all had to log in again (owner's
  report, 2026-10-29). Parking needs tab ids that are unique across
  spaces (`NEXT_TAB_ID` is process-wide; `TabSpace::new` no longer
  counts from one), because the browser host, the open-tabs registry
  and the thumbnails all key on the id. A parked browser tool keeps its
  hub entry (its `Drop` is what removes it), so the host hides its views
  rather than destroying them.

- **Shell (2026-10-15).** The first tab of an activity becomes its home tab (`TabSpace::open` asks first). A dashboard home draws whatever it was given, so an `OpenTool` request into an activity the user had never opened was silently dropped ("View Logs" showed the Logs dashboard, not the source). `open_tool_in` now lets the first tab be the dashboard and opens a working tab beside it. Test cross-tool handoffs against a fresh workspace.
- **Theme.** `Theme::apply` used to call `ctx.set_visuals`, which writes only the visuals slot of egui's current theme. With `ThemePreference::System`, Windows 10 reports the theme after the first frame, egui switched to default light visuals, and a saved dark scheme looked lost. Use `hs_ui::theme::install`. Pin the preference with `ctx.set_theme` and fill both slots with `set_visuals_of`. Never call `set_visuals` directly.
- **Mandatory tabs.** Home and Settings are pinned tabs (`app::MANDATORY`). They're stripped from saved layouts and re-created by `ensure_mandatory_tabs`. Anything that counts, closes, or unpins tabs must skip them.
- **Tab titles.** `title()` is asked every frame and gets no `Services`, so `Action::SetTitle` is overwritten the moment it lands. A tab whose label comes from a stored profile must keep the name in its own saved state.
- **`hard_right` is only for toolbars.** `hs_ui::toolbar::hard_right` draws into a corner the shell reserves in `ui.data`. Called anywhere else (for example a settings form right-aligning Remove), it draws into the wrong toolbar and lays out the page around a widget that isn't there. The symptom was the Project List card rendering as an empty rectangle. The shell now clears the marker after each toolbar, so a stray call falls back to inline layout. Use a plain `with_layout(right_to_left)` in forms.
- **`hard_right` in tool toolbars.** The shell uses `horizontal_wrapped`, so a bare `with_layout(right_to_left)` in a hard-right group only gets the width left on the current row. The AI assistant control vanished first in a narrow window. Use `hs_ui::toolbar::hard_right`.
- **Panel width.** Tie `size_range`'s upper bound to `available_width()` and give way below a floor. Three stacked minimums (props + source + assistant = 800 px in the diagram editor) left the canvas at zero.
- **`horizontal_wrapped` and frames.** It wraps on the size a widget reports before drawing, so a row of `egui::Frame` cards runs past the panel. A nested `Ui` in a horizontal layout is unbounded too. Give the column a real width with `set_width` and compute the columns yourself: `floor((w + spacing) / (card + spacing))`. `ui.horizontal` centres items vertically, so unequal-height cards stagger. Use `horizontal_top`.
- **Escape drops focus.** egui drops keyboard focus on Escape before the tool runs, even when the tool consumes the key (closing a completion list). Request focus for the editor again after handling it.
- **`request_focus()` every frame** re-grants focus, and the field then never reports `lost_focus()` after Enter. Request it once, when nothing else has focus.
- **Plurals.** Fluent `[one]` selectors don't match string arguments. `I18n::t_args` passes canonical integers as numbers, so "1 row" and "1 tab" work. Non-canonical text ("007", "0.12.1") stays text.
- **Double layout passes.** egui lays a frame out twice when a panel's size changes (`request_discard`). `Response::clicked()` is pointer state, so the second pass sees the same click. A detail block that appears on a click therefore receives that click. Selecting a Compose project ran `config`, and the same path on the container list would have reached Remove. Store `ui.input(|i| i.time)` when the selection changes, and refuse verb clicks while it matches. Any pane where selecting a row reveals buttons needs this.
- **Buttons that re-fill their own input** (assistant quick actions) fire twice. Dedupe by `ui.input(|i| i.time)`, which is the same value in both passes. Key events don't repeat. Clicks do.
- **Output belongs to what produced it.** The Compose output block survived a selection change, so one project's `config` appeared under the next. Clear output when the selection changes.
- **Drag thresholds.** `Response::drag_started()` fires only once the pointer passes egui's drag threshold. `interact_pointer_pos()` is then a few pixels from where the button went down. Hit-test grips and handles with `ui.input(|i| i.pointer.press_origin())`. Otherwise an edge drag reads as "move". The same applies to range selections on a strip (`tool-graphics::crop_overlay`, `tool-logs` chart).
- **Never nest context locks.** `ui.data_mut(|d| … ui.input(|i| …))` takes the write lock twice. In a debug build epaint panics after ten seconds ("Failed to acquire RwLock write"). Read `ui.input` into a local first, then call `data_mut`.
- **Double-click tests.** `egui_kittest` synthetic clicks (and raw pointer events) never form a double-click. Cover double-click paths in the running app.
- **Colour blending.** `Color32` is premultiplied. Composite with `base * (1 - a) + over`, not `base * (1 - a) + over * a`. The old `hs_term::widget::blend` dimmed overlays twice, leaving selection and search hits barely visible (fixed 2026-10-16).
- **Caching derived data.** An immediate-mode `ui()` that recomputes each frame hides until data grows: `layout_approx` on a 500-node diagram, a Myers diff of two pasted configs, a regex with a 4 MiB DFA budget. Cache on a hash of the inputs (`diagram::fingerprint`, `input_key`). Test with a large document.
- **Icons.** Check a Nerd Font glyph by looking at it. The codepoint being in the font says nothing about the picture. `U+EBCA` is a cube in JetBrains Mono, not a terminal. `U+E7B0` is a thin unrelated mark, not Docker. Run `scratchpad/collide.py` before choosing a glyph, then look at the result. Use the glyph checks on screen before shipping.
- **Shortcuts and text input.** `egui-winit` turns Ctrl+V, Ctrl+Shift+V, Shift+Insert into `Event::Paste` and Ctrl+C/X into `Copy`/`Cut` before key handling. Never bind them as shortcuts. The Markdown preview is Ctrl+Shift+M for that reason.
- **Global shortcuts win over tool keys.** `run_action` forwards unknown action ids (`file.save`, `file.open`) to the active tool's `run_command`. A tool must not handle the same combo itself.
- **Right-click copy/paste** must work in the terminal and every text field we draw, per the PuTTY convention.
- **Ctrl+Shift+Enter in wrapped text fields** is unreliable with egui. Test it in the running app.
- **Text fields inside an `egui::Grid` collapse.** `TextEdit::desired_width`
  is only a wish: inside a grid cell the leftover width is tiny and every
  key and value renders as an ellipsis. Measure the row once
  (`ui.available_width()`) and place each field with `add_sized`, the way
  `hs_ui::kv_editor` does. The `.env` editor shipped with this bug and was
  caught on screen, not by a test (2026-10-31).
- **Fixed slots in a row.** Two ways to reserve width, both with a catch.
  `allocate_ui_with_layout` allocates only what the child *used*, so call
  `ui.set_min_width(w)` inside it or the slot collapses to the content
  (the snippet list's name and language columns). An empty slot made with
  `add_space(w)` is `item_spacing.x` narrower than a widget of width `w`,
  so add that gap too, or the column after it shifts on the empty rows
  (the `.env` editor's Show/Hide). A slot narrower than the widget it
  holds just grows, which breaks the alignment the slot was for.
- **Vertical centring in a row.** A cell laid out with `ui.horizontal`
  hugs its content and sits at the top of a taller row. Give the cell the
  row's height and centre it:
  `allocate_ui_with_layout(vec2(w, ROW_H), Layout::left_to_right(Align::Center), ..)`
  plus `ui.set_min_height(ROW_H)`.

## Tool-Specific: Terminal and Editor

- **`hs_core::paths::background_command`** is required for any internal command (see Windows section).
- **Locale parse errors** are silent unless the gate runs the locale test (see Patch Scripts).
- **`egui_kittest` (`crates/hs-editor/tests/view.rs`)** covers mouse paths headlessly. Use it alongside the running-app check.

## Tool-Specific: Databases (tool-db)

- **Postgres cancel.** A cancel request issued when a row limit is reached must be awaited before the next statement, or it can cancel that statement. Inside a transaction block it aborts the transaction, so the rest of the result must be read and dropped instead.
- **MySQL `KILL QUERY`** goes out and is awaited before draining the result.
- **Postgres `simple_query`** returns every value as server text (the psql form), which is what the grid wants. Column types come from a separate prepare, used only for alignment and unquoted numbers.
- **Transaction detection.** `driver::tx_effect` (the BEGIN/COMMIT text classifier) can't see a block opened by a function, closed by a failed statement, or left by `ROLLBACK TO`. Postgres and MySQL ask the server after each statement (`probe_in_tx`: `now() <> statement_timestamp()` / `25P02` on Postgres, `@@in_transaction` or the performance-schema row on MySQL). Keep the classifier only as fallback.
- **MySQL/Postgres in tests.** Container-backed driver tests run only when the integration environment variable is set.
- **MySQL ignores a column-level `REFERENCES`.** `parent_id int references parent(id)` parses and is silently dropped, so InnoDB creates no constraint and nothing shows in `key_column_usage` - the FK badge and `table_schema`'s foreign keys both come back empty. Only a table-level `FOREIGN KEY (...) REFERENCES ...` creates one. PostgreSQL and SQLite honour the inline form, so a test written against them passes there and quietly tests nothing on MySQL; `tests/drivers.rs` had exactly that for months, unnoticed because its live servers were not running. Found 2026-10-31.
- **SQLite reports no NOT NULL on a rowid alias.** `pragma_table_info` leaves `notnull = 0` for an `INTEGER PRIMARY KEY`, because the column is a rowid alias rather than a declared constraint. Postgres and MySQL both report a key column as `NOT NULL`, so comparing a SQLite schema against either made *every* primary key read as a difference. `table_schema` records a key column as `NOT NULL` whatever the pragma says. Found in phase H, 2026-10-31.
- **SQLite keeps `AUTOINCREMENT` only in the stored DDL.** No pragma reports it; read it from `sqlite_master.sql`, and accept it only on an `INTEGER PRIMARY KEY`, which is the only place SQLite allows it. A composite key is never one.
- **MySQL stores a string default unquoted.** `information_schema.COLUMNS.COLUMN_DEFAULT` gives `abc` for `DEFAULT 'abc'`, so writing it straight into generated DDL emits an identifier, not a string. Quote anything that is neither numeric nor flagged `DEFAULT_GENERATED` in `EXTRA`. The driver also decodes NULL and the empty string alike, so an empty default cannot be told from none.
- **Composite keys and index columns need an explicit order.** Postgres `indkey` is an array: join it with `unnest(…) WITH ORDINALITY` and sort by the ordinality, or a two-column key comes back in catalog order. MySQL has `STATISTICS.SEQ_IN_INDEX`; SQLite's `pragma_table_info.pk` is the 1-based position in the key, not a flag.
- **Foreign keys must be matched on columns, not names.** SQLite does not name them at all (the pragma gives an `id`), and two servers often name the same relationship differently, so a name-based compare reports differences that are not there.

## Tool-Specific: Logs (tool-logs)

- **Logs with a Searcher.** `has any tool seen this?` goes through `hs_core::search`. Tools implement a `Searcher` over their own store, registered by the binary. Neither side learns the other's schema.
- **Excluding a source vs skipping a searcher.** `ask_all(skip)` drops a whole searcher. `Query::exclude` drops one source. The asking Logs tab needs the second. The first would stop every other Logs tab from answering.
- **Process-wide registries** make tests order-dependent. They pass with `--test-threads=1` and fail in parallel. Serialise with a test-only mutex guard held for each test body (`tool_logs::searcher`).
- **Hold tab records weakly.** Use `Weak<Mutex<Shared>>` in long-lived registries. A strong handle keeps a closed tab's record window alive for the whole session.
- **Histogram buckets** must follow the span, in round units. A fixed per-minute bucket is useless for a week (10 080 bars in 300 px). `chart::Bucket::pick` returns the smallest round unit under `TARGET_BARS`. A computed bucket like "37 seconds" makes the axis unreadable.
- **Time-range filters** replace the after/before chips. Two overlapping time windows combine with AND and filter to nothing, which looks like a broken feature.
- **Record identity.** A record is reached by line number, never by filtered position. The window drops old records, so a position means a different record later. `GOTO_MIME` scrolls to a line. `OpenRequest::File` would reopen the source and lose the place.
- **Logs over SSH** use `hs_ssh::SshClient::exec`. Wait on this path in the running app.

## Tool-Specific: Containers (tool-containers)
- **"could not reach the engine" against a WSL host leaves three
  suspects** - `wsl.exe`, the distro name, `docker` on that distro's
  PATH - and the pane used to name none of them. A read error carries
  the exact command line now (`Command: wsl.exe -d Ubuntu-24.04 --
  docker ps ...`), which can be pasted into a terminal. The same
  profile the owner reported as failing (WSL, Ubuntu-24.04) lists 14
  containers here, so the next report needs that line.

- **Compose and the container list** share the verb-click risk (see Double layout passes under Shell and UI Layout).
- **Output must be cleared** when the selected item changes.

## Tool-Specific: Graphics and Diagrams

- **Crop overlay drag.** Use `press_origin()` for hit tests, not `interact_pointer_pos()` (see Drag thresholds above).
- **Diagram layout and fingerprint** must be cached (see Caching above).
- **Diagram editor width** must respect the panel-width rule (see Panel width above).
- **ComfyUI `/object_info`** describes combo inputs three ways: `[[names], {}]` (classic), `["COMBO", {"options": [names]}]` (V3 schema, 2026), and `{"options": [names]}`. Read all three (`ai_local::combo_options`). Reading only the first returned an empty list and reported "ComfyUI has no upscale models" against a server that had them. The mock test answers in the V3 shape so the old reader can't return.

## Tool-Specific: AI Assistant (hs-core::ai)

- **1min.AI is not OpenAI-compatible.** It's `POST https://api.1min.ai/api/chat-with-ai` (`?isStreaming=true` for SSE), with an `API-KEY` header (no `Bearer`), and body `{"type":"UNIFY_CHAT_WITH_AI","model":…,"promptObject":{"prompt":…}}`. The older `CHAT_WITH_AI` type on `/api/features` is gone. It takes one prompt, so the conversation is folded into a transcript. There's no model-list endpoint and no tool calling, which is why edits use SEARCH/REPLACE blocks.
- **1min.AI docs and chatplayground.ai** don't resolve from the cloud sandbox. Test against a local mock server that speaks both SSE formats. The OpenAI one ends with `data: [DONE]`. The SSE stream sends content pieces, then a result record repeating the whole answer. Ignore that record unless nothing streamed.
- **Diagnosing "the app hangs" on an assistant.** The request runs on the runtime, so the UI thread isn't blocked. What's missing is any sign of time passing. `http_client`'s `read_timeout` only fires on a silent connection, not a service grinding through a huge prompt. Ask which providers fail before suspecting the code. Two providers sharing one request path (DeepSeek and LM Studio, both `openai_body`) where only one hangs point at the payload, not the body shape.
- **Never send a picture to a model that can't read one, and never send a big one.** A 1024 px PNG at quality 100 was about 2 MB, or 2.8 MB base64 (`tool-graphics/tests/attachment.rs`). A text-only model may tokenise that as ~700 k tokens, which looked like a hang on LM Studio with qwen3-coder. Use `ai::VisionSupport` (Auto/Yes/No per provider), with `model_reads_images` as the Auto rule. It answers no for anything it doesn't recognise. A missing description is a better failure than a hang.
- **Image size.** PNG while under 400 KB (screenshots and diagrams, where JPEG smears small text). JPEG above 400 KB (photos). PNG always when there's transparency.
- **Review-only assistants** never run the code they produce. Insert and Run exist only for tools that execute.

## Tool-Specific: API Client (tool-api)
- **A script that throws on line one writes nothing, and that reads as
  "variables are not saved".** `pm.environmentVariables.set(...)` is
  not a Postman API, so the shim had no such object and the TypeError
  stopped the script before the `pm.globals.set` on the next line ran
  (owner's report, 2026-10-29). Aliases people guess at are cheap;
  provide them rather than letting a name decide whether anything at
  all is written.
- **Models do not keep to the tag the prompt names.** `#[serde(tag =
  "op")]` refused `"action"`, `"type"`, `setUrl`, `set-url` and a bare
  `{"method":..,"url":..}` with "missing field `op`" and Apply did
  nothing. Normalise before deserialising (`assist::normalise`) - the
  schema is for the client's benefit, not a test the model has to pass.

- **Partial results.** Keep Postman compatibility: the `pm` scripting object must match Postman's shape.
- **Multipart and forms.** Build them by hand with `send::form_body` and `send::multipart`. The workspace reqwest has no `form` or `multipart` feature.

## Tool-Specific: Browser (tool-browser + browser_host)
- **A WebView2 child holding native focus makes egui refuse focus to
  every widget.** winit reports the *main window* as unfocused the
  moment a child HWND takes focus (WM_KILLFOCUS on the main window),
  and egui's `Memory::begin_pass` drops the focused widget whenever
  `raw_input.focused` is false - so a click on the address bar drew a
  field that could never hold a caret, and the tool's own "address bar
  focused" signal never fired at all (2026-10-29). Nothing the *tool*
  can say fixes that; the host has to put native focus back on the
  main window (`SetFocus`) when a click lands on egui rather than on a
  view, and keep the views disabled (`EnableWindow(false)`) while any
  egui text field holds focus. Read the state from `ctx` -
  `egui_wants_keyboard_input()`, `pointer.any_pressed()`,
  `input.focused` - not from commands the tool may never get to send.
- **A view's child window appears after `create` returns.** A disable
  sent in the frame that created the view lands on no HWND; it has to
  be re-applied on later syncs, and egui has to be kept repainting for
  a moment after a creation or it goes idle before the window exists
  (`settle_until`). This is why New Tab specifically lost the keyboard.
- **`tracing` lines that never appear usually mean the path never ran,
  not that logging is broken** - but check the file is growing first:
  the harness's `stop` is a hard kill and the non-blocking appender's
  last lines go with it. A once-per-two-seconds heartbeat at the top of
  the suspect function tells the two apart in one run.

- **WebView2 paints above egui.** Hide the view whenever a modal, the palette, or a popup is open (`overlay` in `app.rs`), and whenever the tab isn't visible. The tool reports its rectangle every frame, and the host clears it after use.
- **JavaScript on/off** is fixed at view creation. Flipping it rebuilds the view (`Cmd` handling in `BrowserHost::sync`).
- **Keyboard focus.** A WebView2 child holds native keyboard focus even when egui's field has it. `SetFocus` on the parent and `wry`'s `focus_parent()` both fail; the child takes focus back on its next message, and hiding the view lost a tab. `EnableWindow(child, FALSE)` works, because a disabled window can't be focused. The host disables it while the address bar has egui focus and re-enables it on blur (`Cmd::AddressFocus`, sent every frame, since one frame's gap lets the view take the keyboard back).
- **`open_devtools`** needs the `devtools` feature (see Releases).

## Tool-Specific: SFTP and Connection Forms
- **"Follow Shell Folder" cannot depend on the remote prompt.** The
  xterm title (`user@host: dir`) is set by a configured bash/zsh prompt
  and by nothing else - a bare `sh`, a root prompt or a minimal
  `.bashrc` never sets one, and the file pane sat still after every
  `cd` (owner's report, 2026-10-29). Read the command the user pressed
  Enter on from the *grid row* (`Session::cursor_line_text`), not from
  the keystrokes: Tab completion, history recall and a paste never go
  through the key encoder, and the row is what the shell will run.

- **SFTP `write()`** needs `create()` for new files (see Build and Dependency).
- **Clipboard copy** from SFTP downloads to `HyperStackData/tmp/` first (see Windows).
- **Connection profile file names.** `ConnectionStore::file_for` derives the file name from the profile's name (slugified). A hand-written fixture saved under any other name is duplicated on the first write, so the store holds two profiles with the same id. Write fixtures through the app, or name the file exactly `slug(name).toml`.
- **Vault-locked forms.** `store_typed_secrets` used to warn and continue when the vault was locked, so the typed password was saved in the TOML. It now returns `false`, and the form must not save. Any new form must check that result before `upsert`.
- **SSH Follow Shell Folder** reads the xterm title (`user@host: dir`). A remote prompt that doesn't set it leaves the file pane where it is.

## Tool-Specific: S3 (tool-s3)

- **moto keeps buckets in memory.** A restarted `moto_server` has no buckets, so the S3 tool shows `NoSuchBucket`. Re-run `seed_s3.py`.
- **moto checks request shape, not signatures.** The SigV4 unit tests against AWS's examples cover signing.
- **S3 without MinIO:** create a venv, `pip install "moto[server]"`, run `moto_server -H 127.0.0.1 -p 5055` with keys `testing` / `testing`, then `HS_S3_ENDPOINT=http://127.0.0.1:5055 cargo test -p hs-core --test s3_live`.
- **Upload and Download** use native file dialogs. Test them through the client tests, not the GUI.

## Tool-Specific: Scratchpad

- **Ctrl+P hands a choice to the tab that listed it, not to the tab that should show it.** `quick_open_items` results come back as `OpenRequest::File` to the contributing tab directly (`qo:<tab>:<id>` in `app.rs`), bypassing `uri_affinity` routing. A tool whose tabs each hold one document must not just load whatever it is handed: the Scratchpad's `open` passes a note it does not hold back to the shell with `cx.open`, or a pick would overwrite another note's tab, unsaved edits included. This cannot loop, because the shell never routes a document to a tab whose affinity is `None` for it, nor to a dashboard home tab.
- **Don't remember where a note's file is; look it up.** With folders a note can move while a tab has it open - another tab's sidebar drag does it. Saving to a cached path would write a second copy where the note used to be. `save` calls `find_note` every time; a cached path is only a hint for the header's timestamps.
- **`download::safe_name("")` returns `"download"`.** Reused for a folder name, an empty name would silently create a folder called *download*. Check for an empty name before cleaning it.

## Tool-Specific: Automation (tool-automation)

- **Jenkins does not answer a trigger with a build.** `build` / `buildWithParameters` return a queue item in `Location`; the build exists only after the quiet period (5 s by default). Read the runs again rather than expecting a number.
- **Jenkins hides console notes in the plain-text log** (`ESC[8mha:<base64>ESC[0m`, serialised hyperlinks). `logText/progressiveText` includes them; strip them before showing the text, never before computing offsets.
- **Jenkins tree queries contain `[`, `]`, `{`, `}`.** reqwest sends them fine; curl needs `-g` (`--globoff`), or it fails with exit code 3.
- **A Jenkins crumb is tied to the session that fetched it.** With a password, send the crumb's `Set-Cookie` back with the POST. An API token as basic auth needs no crumb at all.
- **Forgejo's `actions/tasks` lists jobs a runner has taken, not runs.** Without a runner a dispatched workflow shows nothing there; the waiting jobs are under `actions/runners/jobs`. Forgejo 11 has no API to cancel a run or read its log.
- **GitHub serves workflow logs only to a token**, even on a public repository (403 "Must have admin rights"). Anonymous calls are also limited to 60 an hour.
- **TeamCity's first start stops at a setup wizard** (data directory, database, licence agreement) before its REST API answers, so it cannot be scripted into a bench container quickly.
- **reqwest 0.13 puts `RequestBuilder::form` behind the `form` feature.** Enable it on the crate that posts forms, not workspace-wide.
- **Windows port 8080 may already be taken** (another Java process here). The bench Jenkins runs on 8091.

## Tool-Specific: Queue (tool-queue)

- **Merging repeated commands must not drop the next one.** `while let Ok(Cmd::Refresh) = rx.try_recv() {}` stops on the first message that is not a Refresh and throws it away; a poll's Refresh followed by Consumers lost the Consumers. Use `hs_core::runtime::next_coalesced`, which keeps it.
- **RabbitMQ's management statistics lag.** `messages_ready` is updated on the statistics interval (5 s by default), so a queue read straight after a publish still shows the old depth. The live tests poll for the depth they expect instead of reading once.
- **RabbitMQ 4 refuses new transient queues** (`PUT /api/queues/…` with `"durable": false` answers 400). Make test queues durable and delete them.
- **RabbitMQ's `guest` user only signs in from localhost**, and a connection through Docker's port mapping is not localhost to the broker. The bench container sets `RABBITMQ_DEFAULT_USER`/`RABBITMQ_DEFAULT_PASS`.
- **JetStream says "no stream found for given subject"** (not only "no responders") when no stream captures a subject. Either means "publish as plain NATS"; the error can come from the publish or from its acknowledgement.
- **Kafka 4 dropped old protocol versions** (KIP-896). A client that sends fixed old versions gets nothing back; `kafka_groups.rs` asks `ApiVersions` first and picks the highest version both sides speak.
- **`OffsetFetch` v10 names topics by id, not name.** Capped at v9 so the response still says which topic an offset is for.

## Headless and GUI Testing

- **Git Bash rewrites absolute paths in arguments to Windows programs**, `wsl` included: `wsl -e docker run … mosquitto -c /mosquitto-no-auth.conf` reached the container as `C:/Program Files/Git/mosquitto-no-auth.conf` and it restarted in a loop. Prefix the command with `MSYS_NO_PATHCONV=1`.

- **Click coordinates.** `click x y` takes client coordinates. `shot` and `rawshot` capture `GetWindowRect`, which includes an 8 px invisible border. A control seen at `(sx, sy)` in a screenshot is clicked at `(sx-8, sy-31)`. Using the screenshot's numbers lands a row or two low. Anything below client height (960 − 39 = 921) fails with "is not over the dev window".
- **Minimum window size.** The window is at least 1720 × 860 (since 2026-10-23). Windows enforces that against user drags but lets other processes' `SetWindowPos` through, so `HyperStackApp::enforce_min_size` pushes back each frame. A harness asking for 1400 × 960 gets the small window for a frame, then 1720 × 860. Take screenshots after a pause, before clicking anything.
- **`focus-hs` resets the window size.** Every command except `rawshot` calls it. To test a resize, do the open and click in one harness invocation, and read the result with `GetWindowRect` from a `SetForegroundWindow` call. Clear topmost afterwards. Use `D:\Temp\focus-hs.ps1`. Don't send the vault password until a screenshot shows the HyperStack window in front.
- **Native dialogs.** The first click after a native file dialog closes only re-activates the window. Click twice, or screenshot after each click.
- **`GetWindowRect` vs client area.** Use `SetProcessDPIAware()` in PowerShell before taking coordinates. Otherwise clicks drift by a row or more.
- **Keyboard via `SendKeys`** works for Ctrl+P, Ctrl+T, and Escape. Synthetic `mouse_event` clicks land only where the screenshot says, after `SetProcessDPIAware`.
- **A stray glyph at the very edge** of a screenshot is the desktop behind the window's invisible border.
- **Headless rfd dialogs don't open.** Seed `layout.json` (editor root and path) instead of clicking Open Folder.
- **Linux/Xvfb.** The app panics at start without `libxkbcommon-x11-0`. Install it, plus `libgl1-mesa-dri`, and it renders with llvmpipe. With no window manager, `xdotool` input is ignored until `xdotool windowfocus <id>`. `xdotool type` needs `--` before text starting with `-`.
- **Xvfb modal dialogs** can leave the window without keyboard focus. Run `xdotool windowfocus <id>` again before typing.
- **Overlay-only checks.** Test a toolbar hard-right layout in a narrow window (`hard_right` in toolbars).
- **`gui-smoke.ps1` takes window-rect coordinates, not client ones.** Its `click` adds the `GetWindowRect` origin to the numbers you pass, and `shot` captures that same rect - so a control at `(sx, sy)` in the screenshot is clicked at exactly `sx, sy`. The `screenshot - (8, 31)` rule recorded for the screenshot sets belongs to the later `hs.ps1` harness, which works in client coordinates. Mixing them puts every click 31 px high, which lands on nothing and looks exactly like the window not being focused. Found 2026-10-31.
- **`$env:TEMP` is not `%LOCALAPPDATA%\Temp` on this machine.** It is `D:\Temp`, so `gui-smoke.ps1`'s scratch data folder is `D:\Temp\hs-gui-data`. Seeding fixtures into the AppData path instead silently does nothing - the app starts fine and simply shows no connections. Read it with `pwsh -NoProfile -c '$env:TEMP'` rather than assuming.
- **A seeded connection profile needs its kind table at the top level.** `ConnectionProfile.fields` is `#[serde(flatten)]`, so a SQLite profile is `[db]`, not `[fields.db]`; a file using the latter parses, loads and contributes no connection. Older hand-written fixtures in the scratch folders still have the wrong shape, which is why they do not appear either.
- **Prefer a context-menu route over typing into the editor.** Driving a feature that reads the editor's statement means getting focus into the editor, and clicking into it does not reliably take focus from the schema tree - the keys land nowhere and the shortcut then does nothing, which looks like the shortcut being unbound. Where the same action exists on a tree node's right-click menu (Compare Data…, Import CSV…), use that: it needs no typing and works first time.
- **A native file dialog leaves focus on the tab that opened it.** After an rfd save or open returns, the next `keys` can reach a different tab than the one you mean. Re-focus with `focus-hs.ps1` and click the intended tab before sending more keys; clicking into an editor does not always take focus from the schema tree.
- **An ERD entity is measured by the label, drawn with its columns.** `hs_diagram::layout::node_size` sized an `Entity` from `node.label` while `build` draws `entity_label(node)` - the table name plus one line per column. Boxes came out one word wide and every identifier wrapped inside them (`customer_id` as "customer" over "_id"). `node_size` now measures what it draws, wraps entities at 260 px and lets their height follow the column count. Any new node kind whose drawn text differs from its label needs the same. Found 2026-10-31.
- **A kittest context starts with no fonts.** Anything drawn with a named family panics with `FontFamily::Name("hs-editor") is not bound to any fonts`. Call `hs_ui::icons::install_fonts(ui.ctx())` on the first frame and return without drawing; draw from the second (see `hs-editor/tests/view.rs`, `hyperstack/src/db_tests.rs`).
- **A `ComboBox` carries its text on a child node,** so `get_by_label("<selected text>")` finds nothing. Reach it with `get_by_role(egui::accesskit::Role::ComboBox)` — `egui` re-exports `accesskit`, so no extra dependency. Its items are `selectable_value` buttons and *are* found by label once the popup is open.
- **Plain `ui.label` text is matched through the node's `value`,** not its label, which kittest handles; a heading or label is therefore queryable, but prefer waiting on a button that only appears in the state you want. `pump` in `db_tests.rs` waits on "Generate Migration" for that reason: it is drawn only once a compare has actually returned differences.
- **A whole tool can be driven headlessly.** `crate::bootstrap::services` on a temp `DataDir` with `NoopWaker` and `KdfParams::FAST` gives real services; `ToolContext` is a plain struct, so frames can be run with `Harness::new_ui_state` while the tool's own tokio session does real I/O. Run a palette command by setting a field the closure takes on the next frame, and poll for the result instead of assuming one frame is enough.

## Security and Vault

- **Secrets in test data.** Any test that writes a secret must grep the data folder afterwards.
- **Vault-locked saves** must return an error (see Connection Forms).
- **Protected environments** default to read-only. Check this when a new tool exposes a write action.

## Cross-Platform

- **A folder name that differs only in case is the same folder.** NTFS and APFS compare names without case, so `Work` and `work` collide on disk. Name-clash checks for anything written to the file system must ignore case - `hs_core::storage::unique_name` does. The older copies in `tool-devtools::snippets::unique_name` (case-sensitive) and `tool-api::model::next_copy_name` are for names that never become paths.
- **Platform code** lives only in `hs-core::secrets`, `hs-core::storage::paths`, `hs-core::clipboard_files`, `hs-core::env_path`, and `hs-term::pty`. Linux clippy catches leaks.
- **BOM-less UTF-16 BE** is ambiguous. Don't guess it during automatic decode. Offer an explicit encoding choice to reopen the original bytes.
- **`canonicalize` on Windows** adds `\\?\` (see Windows).
- **Platform-specific test paths** (upper-case `SystemTemp`) need canonical or case-insensitive comparison (see Release Pipeline).

## Logs and subprocesses

- **A PowerShell script handed to `powershell -Command "..."` through
  `cmd /C` is printed, not run.** `cmd` passes the quotes on, PowerShell
  sees one double-quoted string expression and echoes it with `$vars`
  expanded to nothing. Use `-EncodedCommand` with the script as UTF-16LE
  base64 (`parse::eventlog_command`) and set
  `$ProgressPreference='SilentlyContinue'` first, or a `#< CLIXML` line
  lands in the output.
