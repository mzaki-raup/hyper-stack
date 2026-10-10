# Tool spec — Devtools drawer

**Crate:** `tool-devtools` · **Priority:** P0 (core utilities) · P1 (network tools, process viewer, snippets, .env editor, scratchpad) · **Status:** draft

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
| **Markdown** | editor + live preview with Mermaid (port), export HTML |

### Encoding and crypto (P0)

| Utility | What it does |
|---|---|
| **JWT** | decode header/payload with claim explanations, `exp`/`iat`/`nbf` as local time with validity status, verify with HMAC secret or public key/JWKS URL (fetched with consent), create/sign a token for testing (HS256/RS256/ES256) |
| **Base64 / Base64url / Hex / URL / HTML entities / Unicode escapes / Punycode** | encode/decode with auto-detect, binary file ↔ Base64, data URI builder |
| **Hash / HMAC / checksum** | MD5, SHA-1/256/512, SHA-3, BLAKE3, CRC32 (port Eden's checksum), of text or files, compare against an expected value, HMAC with key |
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
| **.env editor** | table view of `KEY=VALUE`, masking for secret-looking keys, compare two `.env` files, import as environment variables (secrets to keyring), export environment as `.env` (non-secret) |
| **Scratchpad** | per-workspace Markdown notes with preview, auto-saved, checklist support |
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
`similar`, `pulldown-cmark` (+ port of Mermaid renderer), `jsonwebtoken`,
`base64`, `data-encoding`, `md-5`/`sha1`/`sha2`/`sha3`/`blake3`/`crc32fast`,
`hmac`, `aes-gcm`, `chacha20poly1305`, `rsa`, `x509-parser`, `uuid`, `ulid`,
`chrono`/`jiff`, `chrono-tz`, `cron` / `croner`, `regex`, `fancy-regex`,
`heck`, `sqlformat`, `fake`, `palette`, `hickory-resolver`, `surge-ping`,
`sysinfo`/`netstat2`, `rustls` for TLS inspection.
