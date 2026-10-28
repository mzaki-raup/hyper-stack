# Tool spec — SSH + SFTP

**Crate:** `tool-ssh` (on `hs-term` and `hs-core::vfs`) · **Priority:** P0 · **Status:** M2 — shell + SFTP dual pane and local port forwards (`Tunnels` service) implemented (`hs-ssh`, `tool-ssh`); remote/dynamic forwards, jump hosts, ssh-config import, broadcast and server monitor remain (P1)

## 1. Purpose

Reach servers: an interactive shell, a file browser with transfers, remote
editing, and the SSH plumbing (tunnels, agents, jump hosts) that the database,
Redis, API, Docker and log tools reuse. Replaces PuTTY + WinSCP, Termius,
MobaXterm for everyday work.

## 2. User stories

- Import `~/.ssh/config`; `web-01` appears with its jump host; one click opens
  a shell with my agent key, and a second tab opens SFTP at `/var/www`.
- Drag a build artifact from the local pane to the remote pane; the transfer
  shows in Jobs with speed and ETA; a dropped connection resumes it.
- Right-click `nginx.conf` → Edit; Ctrl+S saves back; the shell tab is right
  there for `nginx -t && systemctl reload nginx`.
- Open the same command on five hosts with broadcast input to check disk
  space everywhere.
- The DB tool connects to `staging-db` through `bastion` without me thinking
  about tunnels.

## 3. Layout

```
┌ sidebar: hosts ─────────┬ main (shell tab) ───────────────────────────────────────────────┐
│ 🔍 filter               │ ▌staging▐  web-01  ● connected  key: id_ed25519 via bastion   ⋮  │
│ ▾ staging               │ ubuntu@web-01:/var/www$ docker compose ps                        │
│   🖥 web-01     ● 2     │ NAME       IMAGE          STATUS                                  │
│   🖥 web-02             │ shop-api   shop/api:1.4   Up 3 hours (healthy)                    │
│   🖥 bastion            │ ubuntu@web-01:/var/www$ █                                         │
│ ▾ prod 🔒               │                                                                    │
│   🖥 prod-web-01        ├────────────────────────────────────────────────────────────────── │
│ ▸ Port forwards (1)     │ SFTP ▾ pane (optional split)                                       │
│ ▸ Saved commands        │ local: D:\shop\dist      │ remote: /var/www/shop                    │
│ [+ Add host]            │ index.html   12 KB       │ index.html   12 KB   2026-10-09  -rw-r--r-- │
└─────────────────────────┴────────────────────────────────────────────────────────────── ┘
```

Two tool kinds in the registry: **SSH shell** and **SFTP browser**. Each tab is
one of them bound to one host; opening both for a host shares the underlying
SSH session (multiplexed channels). A toolbar toggle shows an SFTP pane under
a shell (split) for the PuTTY+WinSCP feel.

## 4. Connections

`kind = "ssh"` profile: host, port, user, auth (`password` SecretRef, `key`
path + passphrase SecretRef, `agent`, `keyboard-interactive`), jump hosts
(ordered list of ssh ids, `ProxyJump` semantics), keepalive interval,
connect timeout, startup command, default remote dir, environment variables to
send, terminal overrides (shell, font, colours), host key policy, legacy
algorithm opt-in, X11 off. Import from `~/.ssh/config` (Host, HostName, User,
Port, IdentityFile, ProxyJump, ForwardAgent) with a diff view on re-import.

Session manager: one `russh` client per (profile, user); channels for shell,
exec (tail, monitor), SFTP subsystem, direct-tcpip (tunnels). Auto-reconnect
with backoff when follow-type consumers exist (logs, tunnels); shells show a
"Reconnect" overlay instead.

## 5. Security

- Host key verification against `known_hosts` (OpenSSH format, shared with
  the user's own file if they opt in); first-connect dialog shows fingerprint
  (SHA-256) and lets the user trust or abort; changed key → red dialog with
  "forget and re-trust". `known_hosts` lives in `HyperStackData/` so trust decisions
  travel with the portable folder.
- Agents: OpenSSH agent on Windows (`\\.\pipe\openssh-ssh-agent`), Pageant,
  `SSH_AUTH_SOCK` elsewhere. Agent forwarding is off by default, per host.
- Keys: `ssh-keygen`-style generation UI (ed25519, RSA 4096), passphrase in
  the vault, "copy public key to host" (`authorized_keys` append over SFTP).
  Key files may live inside `HyperStackData/keys/` (referenced as `${DATA_DIR}/keys/…`)
  so a portable folder carries its keys; the folder is created with
  owner-only permissions where the filesystem supports it.
- Protected environments: SFTP delete/overwrite confirmation names the host;
  shell is not filtered but the frame and banner stay red. (Implemented
  2026-10-22: the tab reports the profile's environment through
  `Tool::environment()`; an upload that would replace a file already in the
  listing asks first on a protected host - the profile's environment, else
  the active one; delete always asks and names the host.)

## 6. Shell

`hs-term` grid (alacritty_terminal): true colour, 256 colour, scrollback
(configurable, default 10 k lines), selection with copy on select option,
right-click paste, Ctrl+Shift+F search in scrollback, clickable URLs and
`path:line` patterns (opens in editor via `sftp://` when it exists), bracketed
paste, OSC 7 cwd tracking (keeps SFTP pane in sync when enabled), OSC 52
clipboard off by default, bell as toast, window title from OSC 0 as tab
subtitle, resize on pane change, Nerd Font glyphs. Multiple shells per host
(tabs); **broadcast** mode sends keystrokes to selected shell tabs (red
"broadcast" badge). Saved commands (snippets) insert or run with `{{var}}`
substitution; a sudo prompt detector offers to paste the stored password
(opt-in per host).

## 7. SFTP browser

- **Implemented layout (M2):** the tab splits its width exactly 50/50 between
  the remote file list (left) and the shell (right). The split is recomputed
  from the tab width every frame so it tracks window resizes, and each pane is
  clipped to its half so a wide file table cannot push the divider or the
  shell. The local-folder pane and the optional separate shell split below are
  still P1.
- Dual pane on `hs-ui::DataTable`: local (any folder) and remote (default
  dir); columns name, size, modified, permissions, owner/group, link target;
  sort/filter; breadcrumb with editable path and `~`, `-` history; hidden
  files toggle; symlink following; bookmarks per host.
- Operations: upload/download (drag between panes, drop from OS, copy/paste),
  rename, delete (recursive with count preview), new folder/file, chmod (octal
  and checkbox dialog, recursive option), chown, duplicate, move within remote,
  compress/extract on the remote via exec (`tar`, `zip` when present), edit in
  editor (`sftp://` VFS), open with local app (download to temp, watch, upload
  on save), properties, copy path/URL, "open terminal here" (shell with `cd`).
- Transfer queue (`Transfers` service shared with S3): concurrency per host
  (default 3), chunked reads/writes, resume by size/mtime check, conflict
  rules (ask, overwrite, skip, rename, newer-only), verify by size (checksum
  via exec `sha256sum` optional), speed/ETA, retry, pause, cancel, history.
- Sync (P1): compare local folder and remote folder (size/mtime/hash), show
  differences, mirror in either direction with dry run (uses the same engine
  as S3 sync).
- Fallbacks: SCP for hosts without SFTP subsystem (P1); FTP/FTPS browser on
  `suppaftp` as a separate `ftp://` scheme (P1).

## 8. Port forwarding (P1)

Local (`-L`), remote (`-R`), dynamic SOCKS5 (`-D`) forwards defined per host
or ad hoc; listed in the sidebar and Jobs with bytes transferred; start on
connect option. The `Tunnels` service exposes `ensure_local_forward(ssh_id,
remote_host, remote_port) -> local_port` to other tools, reusing an existing
forward when present.

*Implemented (local forwards):* the trait is `hs_core::tunnels::Tunnels` on
`Services`, implemented by `hs_ssh::tunnels::TunnelManager` and installed at
start-up. One SSH session per profile is shared by every forward through
it (and reconnected on the next connection if it dropped); each forward
listens on `127.0.0.1` only and opens a `direct-tcpip` channel per accepted
connection, counting connections and bytes both ways. `known_hosts` is
re-read at each connect, and a background forward never prompts: an
untrusted host fails with "connect once in the SSH tool to trust its key".
The SSH tool's sidebar lists every forward (local port → remote host:port,
the host it goes through, open connections, ↑/↓ bytes; click copies
`127.0.0.1:port`, × stops it) and has a **New Tunnel** form (`ssh.tunnel.new`
in the palette): host, remote host (default `localhost`), remote port,
local port (empty = any free port). Switching workspace stops all forwards.
Remote (`-R`) and dynamic (`-D`) forwards, start-on-connect and Jobs
entries are still to come.

## 9. Server monitor widget (P1)

Per host, a sidebar/dock panel polling over exec (`/proc` reads or `top -bn1`,
`df -h`, `free -m`, `uptime`, `ss -tulpn`): CPU, memory, load, disks, network,
top processes, listening ports; 5 s interval; sparklines; "kill process" and
"tail this log" shortcuts. Works on Linux and BSD; Windows hosts via
PowerShell remoting are out of scope.

## 10. Integration points

- `sftp://host-id/path` VFS scheme for the editor, logs (`LogSource::Ssh`),
  project tree rooted remotely, drag to S3 (stream through the client).
- `OpenRequest::Terminal{cwd, host}` from any tool.
- Docker tool over SSH, DB/Redis/API tunnels, log tail (`tail -F` exec).

## 11. Keyboard

| Shortcut | Action |
|---|---|
| Ctrl+Shift+T | new shell on same host |
| Ctrl+Shift+F | search scrollback |
| Ctrl+Shift+C / V | copy / paste (Ctrl+C stays SIGINT) |
| Ctrl+Alt+S | toggle SFTP pane |
| F5 | refresh pane |
| F2 | rename |
| Delete | delete (confirm) |
| F4 | edit in editor |
| Ctrl+Shift+B | toggle broadcast |
| Ctrl+Alt+P | port forwards |

## 12. Tests

## 13. AI assistant (implemented 2026-10-14)

The SSH toolbar's **AI Assistant** button offers the same quick actions as
the terminal (Explain Output, Explain the Error, Suggest a Command,
Summarise) and the same **Insert** / **Insert and Run** on a block, which
types it into the remote shell with or without the Return.

It opens help for the SSH host,
connection state, remote folder and visible shell screen (up to 80 rows). It
does not run shell commands, perform SFTP actions, or inspect hidden data.
The context preview lets the user check what will be included; provider access
happens only after a question is submitted.

`russh` integration against an OpenSSH container (password, key, agent, jump,
host key change), SFTP operations and resume after a forced disconnect, config
importer fixtures (including `Include`, `Match` ignored with a note), terminal
escape handling via alacritty's own tests plus snapshot tests of the grid.

## 14. Clipboard files (implemented 2026-10-16)

The remote pane takes files and folders from the system clipboard and gives
them back, so the file manager and the SFTP pane share one gesture.

- **Copy** on the selected entry fetches it — a whole folder tree if it is a
  folder — into `HyperStackData/tmp/sftp-copy-<id>/` and puts that path on
  the clipboard as `CF_HDROP`. The clipboard can only carry paths that
  exist, so the download has to happen first; it runs as a cancellable job
  like every other transfer, and the scratch folder is inside the portable
  data folder (ADR-017), never `%TEMP%`.
- The pane's **paste** button (toolbar, next to Upload) uploads whatever the
  clipboard holds into the folder on screen, recursively, keeping each
  folder's shape. Its tooltip says when the clipboard holds no files.
- Both are palette commands: *SSH: Copy to Clipboard*, *SSH: Paste From
  Clipboard*.
- `hs-core::clipboard_files` encodes and decodes the `DROPFILES` block (a
  20-byte header and double-NUL-terminated UTF-16 paths) with tests; only
  the three clipboard calls around it are platform code. On Linux and macOS
  the module compiles and `write` returns "implemented on Windows only" —
  there is no format a file manager there would read back.

**Why there is no Ctrl+V.** egui-winit answers Ctrl+V by reading the
clipboard as *text* and only then emitting `Event::Paste`; a clipboard that
holds files and no text produces no event at all, so the gesture would work
for some sources and silently do nothing for Explorer. The buttons read
`CF_HDROP` directly and always work.

## 15. The file pane's actions (2026-10-17)

Everything that can be done to one entry is in its **right-click menu**,
not in a bar under the listing: Open in Editor and Download for a file,
Rename, Copy (to the system clipboard), Copy Path, Delete, **Permissions…**,
and Open Terminal Here for a folder, with separators between the groups.
`hs_ui::table` reports the right-clicked row (`TableResponse::secondary`)
and the pane draws the menu itself, so the table keeps no opinion about
what a row means.

Double-clicking the empty space below the listing goes up a level, the way
a file manager does. The toolbar has **New File** beside New Folder, which
creates an empty file and refuses to flatten a name that is already there.

**Download Zipped** fetches the entry into the data folder's scratch
space and packs it with `hs_core::archive` as `.zip`, `.tar.gz` or `.tar`
— the user picks from the submenu, and `.zip` is first because it is what
Windows opens unaided. Entries go under one top folder so unpacking never
scatters files, `.git` is skipped, and the scratch copy is removed whether
the packing succeeded or not. The listing draws the Material file-type
icons from `hs_ui::fileicons`, the same set the editor's project tree uses.

### Permissions

The dialog shows the owner, the group, the mode as a number and the same
mode as nine boxes; typing a number ticks the boxes and ticking a box
changes the number, and the `rwxr-xr-x` form is echoed under them. Only
the low nine bits are edited — setuid, setgid and the sticky bit are left
exactly as they were. `tool-ssh::perms` holds the arithmetic and is tested
over every one of the 512 modes.

The mode is written with an SFTP `setstat`, which every server allows
without a shell. **Owner and group are not**: SFTP v3 carries numeric ids
and the change usually needs root, so those are sent as a `chown` into the
shell, where the user sees the command run and reads any error it gives.
The listing shows whatever the server named — a name when it sends one, the
numeric id when it does not, and blank rather than a guess.

## Addendum (2026-10-17): file actions show their command in the shell

**Follow Shell Folder** now keeps both halves of the tab telling the same
story. Opening a folder in the file pane still runs `cd` for real, because
the shell's own working directory has to move. Every other SFTP action —
New Folder, New File, Rename, Delete, Permissions — writes the command for
what just happened into the shell as a **comment**:

```
dev@testbox:/home/dev$  # mkdir '/home/dev/releases'
dev@testbox:/home/dev$  # touch '/home/dev/notes.txt'
dev@testbox:/home/dev$  # chmod 646 '/home/dev/notes.txt'
dev@testbox:/home/dev$  # rm '/home/dev/notes.txt'
```

A comment, not a command, and deliberately so: the action has already
happened over SFTP, and running it a second time would at best be wasted and
at worst destroy something (`rm` twice on a name reused in between). The
shell swallows a `#` line and redraws its prompt, so the echo costs nothing
and leaves the line discipline alone — which injecting text straight into
the grid would not. Owner and group changes were already real `chown`
commands and stay that way, because there is no SFTP operation for them.
Owner's requirement, 2026-10-17.

## Dashboard (2026-10-29)

The activity's home tab wears the shape every activity now shares
(`hs_ui::dashboard::saved_dashboard`): a band with the activity's
mark, name and subtitle; the figures in equal rounded boxes;
**Start Here** with its first moves; **Saved Hosts** - every saved host over the `user@host:port` it would open;
and the shared **Open Here** grid, each section ruled apart.
