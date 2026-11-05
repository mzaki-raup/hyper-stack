# Product logos

Multi-coloured product logos from **Devicon**
(<https://github.com/devicons/devicon>, MIT, licence text in `LICENSE.txt`),
tag **v2.18.0**, the `<name>-original.svg` of each (`-plain.svg` where a
logo has no original). They are what every connection list, tab, dashboard
and work-pane header draws for a connection kind, an SSH host's operating
system, a terminal shell or CLI, and an Editor project's language
(`hs_ui::icons::conn_icon`, `os_conn_icon`, `language_conn_icon`,
`cli_conn_icon`; CR-0036). The logos remain the trademarks of their owners.

| Use | Files |
|---|---|
| Container hosts | `docker` `podman` `kubernetes` `windows11` |
| Databases, by engine | `postgresql` `sqlite` `clickhouse` `duckdb` `microsoftsqlserver` `oracle` `mysql` `mariadb` |
| NoSQL, Redis, search | `redis` `mongodb` `couchdb` `elasticsearch` `cassandra` `dynamodb` |
| Brokers | `rabbitmq` `nats` |
| Build servers | `jenkins` `teamcity` `githubactions` `gitlab` `forgejo` `ansible` |
| Trackers | `jira` `confluence` |
| SSH hosts, WSL shells | `ubuntu` `debian` `fedora` `redhat` `centos` `archlinux` `opensuse` `linux` `windows11` `apple-rainbow` |
| Shells | `powershell` `bash` `zsh` `fish` `git` |
| CLI tools | `azure` `googlecloud` `nodejs` `python` `terraform` `claude` `dotnetcore` (and the database and container logos above) |
| Languages | `go` `c` `cplusplus` `csharp` `java` `kotlin` `scala` `swift` `zig` `haskell` `elixir` `dart` `python` `ruby` `php` `perl` `javascript` `typescript` `nodejs` `bash` `terraform` `html5` `css3` |

**`apple-rainbow.svg` is ours, derived from Devicon's `apple-original.svg`**
(MIT): the same outline, filled with the classic six stripes (green, yellow,
orange, red, purple, blue) instead of black, which is a hole on the dark
theme.

Rust, Lua and Markdown use the **Material Icon Theme** files already in
`assets/icons/material/` (MIT): Devicon's are black or navy.

Not vendored, on purpose:

- **Black or near-black logos** vanish on the dark theme. GitHub, Apache
  Kafka, MySQL and MariaDB keep their glyph, drawn in the theme's text
  colour (MySQL's blue dolphin is a thin dark outline; MariaDB runs through
  the same engine).
- **No Devicon logo**: S3, Qdrant, MQTT, Redmine, Alpine, FreeBSD and
  Nushell keep their glyph in the product's colour (MQTT's a lighter step of
  its `#660066`, which is lost on the dark theme). Valkey, WebDAV, Nexus and
  Windows' Command Prompt keep their glyph as it was.

To add one: copy `icons/<name>/<name>-original.svg` from the same tag into
this folder, add it to `PRODUCT_LOGOS` in `crates/hs-ui/src/icons.rs`, and
map it. Measure its fills and look at it in both themes before keeping it.

## `../simple/`: Simple Icons marks

Marks Devicon does not carry come from **Simple Icons**
(<https://github.com/simple-icons/simple-icons>, CC0 1.0, v13 / latest,
2026-11-06), each filled with the brand colour the same data file names,
because the originals are single black paths: `puppet` (#FFAE1A), `mqtt`
(#660066), `amazons3` (#569A31), `qdrant` (#DC244C) and `redmine` (#B32024;
Redmine's mark is CC BY-SA 2.5, credited to the Redmine project). Valkey,
WebDAV and Sonatype Nexus have no usable mark there (Valkey is absent, the
Sonatype mark is near-black) and keep their glyphs. The marks remain the
trademarks of their owners.

The AI Assistant's services (CR-0055) draw from the same folder: Simple
Icons marks for `openai` (#10A37F), `anthropic` (#D97757), `googlegemini`,
`amazonwebservices` (#FF9900), `mistralai`, `deepseek`, `ollama` (grey, the
mark is black), `openrouter`, `moonshotai`, `tencentqq` (Hunyuan), `vllm`,
`lmstudio` and `windsurf`, Devicon's `azure`, and drawn look-alikes for the
services with no published mark: `comfyui` (a node graph), `sdwebui`
(diffusion rings, for AUTOMATIC1111, SD.Next and Forge), `swarmui` (three
hexagons), `onemin` (a teal one), `fal` (a violet tile with a bolt),
`invokeai` and `fooocus`. The look-alikes are ours and carry no trademark.
