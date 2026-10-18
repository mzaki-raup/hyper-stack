# Tool spec — Devtools drawer

**Crate:** `tool-devtools` · **Priority:** P0 (core utilities) · P1 (network tools, process viewer, snippets, .env editor, scratchpad) · **Status:** first utilities implemented (2026-10-15); see §0

## 0. What is implemented (2026-10-15)

One tool with the utilities listed in its sidebar by category; **each
utility has its own tab** (`devtools://<key>`): clicking a utility in the
sidebar, or its palette command, focuses the tab that shows it or opens one.
The utility list is a vertical scroll area under the fixed header, so a short
window or more utilities never hides the last group. Owner's request,
2026-10-19.
Inputs are part of the tab's `save_state` and come back after a restart,
with one rule: a key, password, token or service account is saved **only
when it is a `{{reference}}`** to an environment variable; a typed secret
is dropped (`for_save()` in each module, `keep_reference`). Every
computation is a pure function with known-answer tests.

Secret fields carry two helpers from `hs_ui::varpeek`:

- **Save to Environment…** moves the typed value into the vault as a
  variable of the active environment (`secrets.create` → `set_var`), leaves
  `{{name}}` in the field and publishes `EnvironmentsChanged`. It needs an
  unlocked vault and an active environment, and says so otherwise.
- **Peek** (the eye, shown when the text references a variable) lists each
  `{{name}}` with its value and a Copy button; vault secrets are masked
  until **Reveal**, and a locked vault is reported instead of a value.

Every output box (`util::output`) has Copy and **Download…**, which saves
the text as a `.json`, `.xml` or `.txt` file named after the box.

Values are expanded when a utility runs (`Services::expand`: `{{}}`
substitution through the environment chain, then `secret:<id>` resolution
through the vault), so a Firebase service account, a PEM key or a Jasypt
password can live in an environment and be shared by every utility. A
missing variable or a locked vault stops the operation with that message.

| Category | Utility | What it does |
|---|---|---|
| Encoding | **Base64** | Decode standard/URL-safe with or without padding or a `data:` URI, auto-detected; the result is shown as a picture, as text, or named as a document (PDF, ZIP/Office, …) with Save As; hex dump; encode text or a file (URL-safe, no padding) with the data URI; Basic authentication header from user and password |
| Encoding | **URL** | Percent-encode and decode in three scopes: **Component** (one query value or path segment: everything outside `A-Za-z0-9-._~` escaped), **Full URL** (the delimiters `:/?#&=@` kept so a link typed with spaces or accented letters becomes usable, existing escapes left alone) and **Form** (`application/x-www-form-urlencoded`, space as `+`); an optional strict mode escapes `-._~` too; invalid escapes are left as they are rather than dropped; a parts table splits a URL into scheme, host, path, each query parameter and the fragment with their values decoded |
| Encoding | **YAML** | Check a document and report each mistake with its line (tabs used to indent, a line that is not `key: value`, a key set twice in one block, an over-indented line, an unclosed quote), and convert **YAML → Properties** and **Properties → YAML**. The supported subset is the one configuration files use: comments, nested blocks, sequences (including lists of maps), quoted and plain scalars, block scalars (`|`, `>`), simple flow sequences and `---`. Anchors, aliases, tags and flow maps are reported instead of guessed at, so a conversion never invents a value. Lists become `key[0]`, `key[1]`; Java's `=`, `:`, space and bare-key forms are all read back. The three tabs keep their own text, so checking a document does not replace what is waiting in a converter |
| Encoding | **Hashes** | MD5, SHA-1, SHA-256/384/512, SHA3-256/512, BLAKE3, CRC32 of text (optionally minified JSON) or a file, as hex or Base64, HMAC with a key, compare field that turns the matching row green; a column table with bit lengths and copy buttons |
| Tokens | **JWT** | Decode with claim explanations (`exp`/`iat`/`nbf` as local time, expired / not yet valid), verify with an HMAC secret or a public key (PEM or Java's Base64), sign a token for testing: HS256/384/512, RS256/384/512, ES256 |
| Tokens | **Firebase Tokens** | From a service-account JSON: custom token for a uid with claims (offline), OAuth 2 access token for the Google APIs (one request), ID token by signing in with the custom token and the Web API key (one request) |
| Cryptography | **Jasypt** | `PBEWithMD5AndDES` (PBKDF1-MD5, DES-CBC) and `PBEWITHHMACSHA512ANDAES_256` (PBKDF2-HMAC-SHA512, AES-256-CBC) with jasypt's `salt ‖ IV ‖ ciphertext` layout (salt and IV sizes adjustable), Base64 or hex, `ENC(…)` wrapper; plus the legacy `DataEncryptor` (DES/ECB with the first 8 bytes of a key string, AES/ECB with a 16-character key, MD5 as Base64) |
| Cryptography | **PKI Messages** | The gateway flow: sign with the sender's RSA key (`SHA256withRSA`) and encrypt with AES-256-GCM under `SHA-256(sharedKeyUuid)` (12-byte IV prepended); decrypt and verify on the other side. Flow 1 (device → server) and Flow 2 (server → device) with the device/target key pairs, producing and reading the `{deviceId, bucketUri, encryptedMsg, signatureStr}` payload |

Text handed over from another tool (`OpenRequest::Text`) lands in the open
utility's input.

## 1. Purpose

The small utilities developers otherwise do on random websites — with
production tokens. Everything runs locally, offline, in a tab or in a quick
popover from the palette, and each utility accepts input from and sends output
to the other tools. Each utility is a tiny `Tool` registered under the
`devtools.*` id prefix so it appears in the launcher and palette
individually ("JWT", "Cron") and can be pinned.

## 2. Catalogue

### Data formats (P0)

| Utility | What it does |
|---|---|
| **JSON** | format (indent options, sort keys), minify, validate with error position, tree view with path copy, JSONPath / jq-like query (`jaq` crate), convert to YAML/TOML/XML/CSV (when array of flat objects), escape/unescape string, JSON ↔ JSON lines, JSON Schema validate (schema paste), generate JSON Schema / TypeScript / Rust `serde` struct from a sample |
| **YAML / TOML / XML** | format, validate, convert among each other and JSON, XPath query for XML, YAML multi-document split |
| **CSV / TSV** | table view with header detection, delimiter sniffing, convert to JSON/Markdown/SQL `INSERT`, column stats |
| **Diff** | two-pane text diff on `hs-editor` Diff mode, char/word/line, ignore whitespace, JSON-aware diff (sorted keys), copy as unified patch |
| **Markdown** | editor + live preview with a native Mermaid subset (flowchart, sequence), export HTML |

### Encoding and crypto (P0)

| Utility | What it does |
|---|---|
| **JWT** | decode header/payload with claim explanations, `exp`/`iat`/`nbf` as local time with validity status, verify with HMAC secret or public key/JWKS URL (fetched with consent), create/sign a token for testing (HS256/RS256/ES256) |
| **Base64 / Base64url / Hex / URL / HTML entities / Unicode escapes / Punycode** | encode/decode with auto-detect, binary file ↔ Base64, data URI builder |
| **Hash / HMAC / checksum** | MD5, SHA-1/256/512, SHA-3, BLAKE3, CRC32, of text or files (streamed), compare against an expected value, HMAC with key |
| **Encrypt / decrypt (P1)** | AES-GCM / ChaCha20 for test vectors, RSA key pair generation, PEM/DER inspect, X.509 certificate decode (subject, SAN, validity, chain) |
| **Password / random** | passwords with rules, random bytes/hex, passphrase (diceware) |

### Identifiers and time (P0)

| Utility | What it does |
|---|---|
| **UUID / ULID / NanoID / Snowflake** | generate (v4, v7, v1/v5 with namespace), decode (version, timestamp from v1/v7/ULID), bulk generate |
| **Timestamp** | Unix s/ms/µs ↔ ISO-8601 ↔ RFC 2822 ↔ human; time zone converter with a pinned list of zones; "now" button; relative time; duration calculator; week number; business days |
| **Cron** | parse 5/6/7-field expressions (Unix, Quartz, AWS, Kubernetes aliases like `@hourly`), English explanation, next 10 runs in a chosen zone, builder with dropdowns, validate against the selected flavour |
| **Number base / units** | dec/hex/oct/bin with bit view, byte sizes (KiB vs KB), colour values |

### Text (P0/P1)

| Utility | What it does |
|---|---|
| **Regex** | Rust `regex` and PCRE-ish (`fancy-regex`) flavours, live highlighting of matches and groups, replace preview, explanation tree, flags, cheat sheet, test strings saved with the pattern, generate code snippet for Rust/JS/Python/Go (P1) |
| **Case / lines** | snake/camel/kebab/title/upper/lower, sort/unique/reverse/shuffle lines, trim, remove duplicates, number lines, word/char counts |
| **SQL formatter (P1)** | format and minify SQL per dialect (`sqlformat`), keyword case |
| **Lorem / fake data (P1)** | names, emails, addresses, phone numbers, UUIDs, dates, JSON fixtures from a template (`fake` crate) |
| **Colour (P1)** | picker, hex/rgb/hsl/oklch conversion, contrast ratio (WCAG), palette shades |
| **String escape** | shell (bash/PowerShell), SQL, JSON, C, regex escaping |

### Network (P1)

| Utility | What it does |
|---|---|
| **DNS** | A/AAAA/CNAME/MX/TXT/NS/SRV/PTR lookups against system or chosen resolver (`hickory-resolver`), DoH option |
| **Ping / TCP / HTTP check** | ICMP where allowed, TCP connect latency, HTTP(S) HEAD/GET with status and timing, repeat mode with sparkline |
| **Port scan (single host, user-owned)** | common ports or range on a host the user names; rate-limited |
| **TLS certificate viewer** | connect and show the chain, SANs, expiry, cipher, protocol, OCSP status |
| **whois / IP info** | whois lookup, IP → ASN/geo via offline database (P2) |
| **Local processes and ports** | what is listening on which port (`netstat`/`ss`/Win32 APIs), process name/PID/cwd, kill (guarded), "open in API client" for HTTP ports |
| **HTTP echo server** | local listener that logs incoming requests (webhook testing), with ngrok-style tunnel integration left to the user |

### Workspace helpers (P1)

| Utility | What it does |
|---|---|
| **Snippets** | code/SQL/shell/command snippets with language, tags, `{{var}}` placeholders, search from quick-open, insert into editor/terminal/console |
| **.env editor** | table view of `KEY=VALUE`, masking for secret-looking keys, compare two `.env` files, import as environment variables (secrets to the vault), export environment as `.env` (non-secret) |
| **Scratchpad** | implemented as its own tool, see [scratchpad.md](scratchpad.md): per-workspace Markdown notes, preview, autosave, pinning, AI assistant |
| **Clipboard history (P2)** | last N text items (opt-in, excluded for secrets copied from the app) |

## 3. Interaction model

- Each utility is a tab or a **popover** (from the palette: "JWT: decode
  clipboard") that shows the result and offers "Open in tab".
- Input sources: paste, type, drop a file, "from selection" of the active
  editor/response/cell/log record (`OpenRequest::Text`), chain output of one
  utility into another (Base64 decode → JSON format → JSONPath).
- Output targets: copy, save to file, send to editor, send to API body, insert
  into terminal, replace selection in the editor.
- All conversions are pure functions in a `devtools-core` module with unit
  tests and known-answer vectors; the UI is thin.

## 4. Privacy

Everything runs locally. The only network calls are the network utilities and
JWKS fetches, which say so before running. Inputs are not saved to history
unless the user pins them; secret-looking inputs (JWTs, keys) are never
written to disk.

## 5. Keyboard

Palette-first: `Ctrl+Shift+P` → type `jwt`, `cron`, `uuid`. In a utility tab:
Ctrl+Enter to run/convert, Ctrl+Shift+C copy output, Ctrl+Shift+O open in
editor, Ctrl+Shift+S swap input/output.

## 6. Crates

`serde_json`, `serde_yaml`/`serde_yml`, `toml`, `quick-xml`, `jaq`, `csv`,
`similar`, `pulldown-cmark` (+ our own Mermaid subset renderer), `jsonwebtoken`,
`base64`, `data-encoding`, `md-5`/`sha1`/`sha2`/`sha3`/`blake3`/`crc32fast`,
`hmac`, `aes-gcm`, `chacha20poly1305`, `rsa`, `x509-parser`, `uuid`, `ulid`,
`chrono`/`jiff`, `chrono-tz`, `cron` / `croner`, `regex`, `fancy-regex`,
`heck`, `sqlformat`, `fake`, `palette`, `hickory-resolver`, `surge-ping`,
`sysinfo`/`netstat2`, `rustls` for TLS inspection.

## Addendum (2026-10-17): generators, TOML and XML

**Generators** (`generate.rs`) covers four jobs behind one page: UUIDs,
random numbers, passwords and passphrases. Everything is a plain function
over its inputs, so the rules are testable without a UI and there is one
visible place that decides what a password is made of.

- **UUID** v4 (122 random bits), v7 (a millisecond timestamp then random
  bits, so ids sort in creation order — what you want for a database key)
  and the nil UUID. The clock is passed in rather than read inside, which
  is what makes v7 testable.
- **Number** takes bounds and a count; bounds the wrong way round are
  swapped rather than refused.
- **Password** guarantees at least one character from every class that was
  ticked — a rule that says "include digits" and then produces none is a
  rule that lied — then shuffles, so those characters are not always in
  the same places. Look-alike characters can be excluded.
- **Passphrase** draws from a 323-word list written here rather than taken
  from a published one, so no licence rides along.

A **seed** makes any generator repeatable, which is the point of offering
one. Leave it empty for fresh randomness.

**Each generator keeps its own result.** The result card is titled with the
generator that produced it ("Password Result") and is stored per mode, so
moving to another generator to make something else does not throw away what
you just generated, and coming back shows it again. One shared card meant a
passphrase replaced the UUIDs you were still copying from. Owner's
requirement, 2026-10-17.

**Entropy is shown, not a word like "strong".** A password's is
`length × log2(pool)`; a passphrase's is `words × log2(list)`. The word
list is worth about 8.3 bits a word against a 7776-word diceware list's
12.9, and the page says so, because a passphrase from a short list looks
far stronger than it is.

**TOML** and **XML** (`markup.rs`) each check first and format second. XML
indents with 2 spaces, 4 spaces or a tab (`fmt::pretty_xml_with`); TOML has
no indent to choose, because it is not an indentation-based format, and the
page says that instead of offering a setting that does nothing. The XML
check is **well-formedness only** — every tag closed and properly nested,
no DTD or schema consulted — and it reports the line and column, naming
which tag is unclosed or which close matches nothing. Broken markup is
never reformatted: tidy-looking output would hide the break.

### Cron (2026-10-17)

`cron.rs` builds an expression from a description of when a job runs — a
frequency, a time, days or a date — and writes it in **all three dialects
at once**: Unix cron, Spring and Quartz. Showing all three is the point,
because the usual reason to open such a page is needing the other
dialect's version of the one you already have.

The differences it handles so nobody has to remember them:

- Spring and Quartz lead with a **seconds** field; Unix cron has none.
- **Quartz refuses `*` in both day fields** — exactly one of day-of-month
  and day-of-week must be `?`. The builder puts it in whichever is not
  being used, and the checker rejects an expression with neither or both.

A pasted expression is checked against each dialect in a three-column
grid — **Cron Type**, **Match** and **Details of Scheduler** — so one look
says which dialects accept it and, for those that do, when it actually
runs: `explain()` reads the expression back as English ("At 09:30, on day
of the week MON,WED,FRI."), and the dialects that reject it say why on the
same row. What is checked is field counts and ranges, including lists,
ranges and steps. Names and modifiers (`MON`,
`JAN`, `L`, `W`, `#`) pass through unchecked, because the dialects differ
on which they accept and refusing them here would be wrong more often than
right — so this says an expression is plausible, not that a given
scheduler will take it.

One test asserts that **every expression the builder writes passes its own
checker**, for all six frequencies in all three dialects. It caught a real
bug: the daily and every-N cases were emitting `*` in both Quartz day
fields, so Quartz output failed Quartz validation.

### Unit Conversion (2026-10-17)

Five panes, each with **its own inputs and its own result**: switching from
Time to Numbers must not leave a figure on screen that the other pane
produced, which is the same rule the generators follow.

- **Time** converts between nanoseconds and weeks. Months and years are
  deliberately absent: neither has a fixed length, so "1 month in hours"
  would have to invent an answer.
- **Epoch** converts a timestamp to an ISO-8601 instant in UTC and back,
  in seconds or milliseconds, with **Now** to stamp the current moment.
  Either field can be the question; the other is the answer, and a date on
  its own is read as midnight UTC.
- **Numbers** reads a value in decimal, binary, octal or hex and shows all
  four, plus the character when the value is a printable code point — a
  hex dump is easier to read when 0x41 says `A`. Grouping underscores are
  ignored, and a `0x`/`0o`/`0b` prefix is accepted only when it agrees with
  the base being read, so `0x10` typed as decimal is a mistake rather than
  silently sixteen.
- **Data Size** keeps the two families apart: kB/MB/GB count in powers of
  ten, KiB/MiB/GiB in powers of two. It opens on GB → GiB, because "why is
  my 1 TB disk 931 GiB?" is the question that brings people to a converter.
- **Duration** reads `1d2h3m4s`, `90m` or a plain number of seconds and
  writes it back as something readable, with the total in seconds, minutes,
  hours and days. A number with a unit missing (`1h30`) is refused rather
  than guessed at.

Time and Data Size also list **every unit at once**, since the conversion
that was not asked for is often the one wanted next. The arithmetic is
plain functions over their inputs, so the rounding rules live in one place
and are covered by tests without a UI. Owner's requirement, 2026-10-17.

### Regex Tester and Text Diff (2026-10-18)

The last two of M5's core utilities, under a **Text** category:

- **Regex Tester** (`regex_tool`): a pattern with the `regex` crate's
  flags (ignore case, multi-line, dot-all, ignore whitespace), text to try
  it on, the text with every match marked, a list of matches with their
  groups by index and name, and a replacement preview with `$1`/`$name`
  expansion. The engine is the one HyperStack's own filters and highlight
  rules use, so a pattern that works here works there; its RE2-like
  syntax (no backreferences or look-around) is what the crate reports
  when one is typed. Pasted text (Accept::Text) lands in the text field.
- **Text Diff** (`diff_tool`): two texts side by side with optional names,
  a line diff from `similar` with deleted and inserted lines marked and
  numbered on both sides, a whitespace-and-blank-lines-insensitive mode,
  Swap Sides, and Copy as Unified Diff. Pasted text fills the empty side.

Both keep their state with the tab. Tests cover groups, flags, errors,
line marking, the whitespace mode and the unified header.
