# HyperStack — English (source locale). Every visible string goes through here.
#
# Casing rule (CLAUDE.md): Title Case for buttons, item names, setting names and
# other labels; sentence case for information and instruction text.

app-name = HyperStack
app-tagline = One workspace for everyone who ships software.

## Categories
category-code = Code
category-data = Data
category-network = Network
category-infra = Infra
category-utilities = Utilities

## Shell
launcher-title = What do you want to do?
launcher-search = Type to search tools, connections and recent items
launcher-recent = Recent
launcher-pinned = Pinned Connections
launcher-no-tools = No tools match.
tab-count = { $n ->
    [one] { $n } tab
   *[other] { $n } tabs
}
tab-new = New Tab
tab-close = Close
tab-close-others = Close Others
tab-close-left = Close to the Left
tab-close-right = Close to the Right
tab-pin = Pin
tab-unpin = Unpin
tab-duplicate = Duplicate
tab-split-right = Split Right
tab-split-down = Split Down
tab-unsaved-title = Unsaved Changes
tab-unsaved-body = "{ $title }" has unsaved changes. Close it anyway?
tab-unsaved-discard = Discard and Close
tab-unsaved-keep = Keep Open
sidebar-toggle = Toggle Sidebar
dock-toggle = Toggle Bottom Dock
dock-output = Output
dock-jobs = Jobs
dock-problems = Problems
dock-terminal = Terminal
jobs-none = No background jobs.
jobs-cancel = Cancel
status-ready = Ready
status-jobs = { $n ->
    [0] no jobs
    [one] 1 job
   *[other] { $n } jobs
}
env-none = No Environment
env-switch = Switch Environment
env-protected = Protected
env-banner-pinned = This tab is pinned to { $name }
workspace-switch = Workspace
workspace-open = Open Workspace…
workspace-new = New Workspace…
workspace-name = Workspace Name
palette-placeholder = Type a command…
quick-open-placeholder = Open a file, connection, query or request…
palette-no-results = Nothing matches.
notifications = Notifications
notifications-none = No notifications yet.
cancel = Cancel
ok = OK
save = Save
delete = Delete
close = Close
yes = Yes
no = No

## Fallback notice
fallback-title = Data Folder
fallback-body = HyperStack could not write beside its executable, so it is using { $path } instead. Settings, workspaces and the vault live there.

## Settings
settings = Settings
settings-general = General
settings-updates = Updates
settings-appearance = Appearance
settings-shortcuts = Shortcuts
settings-environments = Environments
settings-connections = Connections
settings-security = Security
settings-data = Data
settings-about = About
settings-startup-workspace = Startup Workspace
settings-restore-tabs = Restore Tabs From the Last Session
settings-language = Language
settings-language-auto = Auto
settings-theme = Theme
settings-theme-dark = Dark
settings-theme-light = Light
settings-font-size = UI Font Size
settings-min-tab-width = Minimum Tab Width
settings-tab-gap = Tab Gap
settings-ui-zoom = UI Zoom
settings-shortcut-action = Action
settings-shortcut-binding = Shortcut
settings-shortcut-conflicts = Conflicts
settings-shortcut-reset = Reset to Defaults
settings-shortcut-invalid = "{ $value }" is not a valid shortcut.
settings-data-folder = Data Folder
settings-data-open-folder = Open Data Folder
settings-data-reset = Deleting the data folder resets HyperStack completely.
settings-vault-status = Secret Vault
settings-vault-locked = Locked
settings-vault-unlocked = Unlocked
settings-vault-lock = Lock Now
settings-vault-relock = Lock After Idle (Minutes)
settings-vault-change = Change Master Password
settings-vault-unlock-first = Unlock the vault to store secret variables.
settings-clipboard-clear = Clear Clipboard After Copying a Secret (Seconds)
settings-updates-channel = Channel
settings-updates-channel-stable = Stable
settings-updates-channel-beta = Beta
settings-updates-auto = Check for Updates Automatically
settings-updates-auto-hint = Once a day, on startup. A single request to git.zacx.net; nothing else is sent.
settings-updates-check-now = Check Now
settings-updates-up-to-date = Up to date
settings-updates-checking = Checking…
settings-updates-last-checked = Last checked { $when }
settings-updates-never-checked = Not checked yet.
settings-updates-available = Update available: { $from } → { $to }
settings-updates-notes = What's New
settings-updates-download = Download ({ $size })
settings-updates-downloading = Downloading { $version }…
settings-updates-ready = { $version } is downloaded and verified (size, hash, signature, self-test).
settings-updates-install = Restart and Install
settings-updates-skip = Skip This Version
settings-updates-view = View on git.zacx.net
settings-updates-manual = A newer release ({ $version }) exists but cannot be installed in place: { $reason }
settings-updates-previous = Previous Version
settings-updates-revert = Revert to { $version }
settings-updates-revert-confirm = Replace the running version with { $version } and restart?
settings-updates-exe-folder = Executable Folder
settings-updates-writable = writable
settings-updates-read-only = read-only
settings-updates-error = Could not check for updates: { $error }
settings-updates-when-found = When an Update Is Found
settings-updates-when-notify = Tell Me
settings-updates-when-install = Download and Install When I Exit
update-toast-available = HyperStack { $version } is available.
update-toast-installed = Updated to HyperStack { $version }.
update-toast-failed-install = The last update to { $version } did not start cleanly. You can revert from Settings → Updates.
update-badge = Update
settings-about-version = Installed Version
settings-about-signing-key = Release Signing Key
settings-about-project = Project
settings-about-license = MIT license · no telemetry · portable

## Environments editor
env-name = Name
env-colour = Colour
env-protected-flag = Protected (Production)
env-protected-hint = Read-only defaults and confirmations before destructive actions.
env-parent = Inherits From
env-variables = Variables
env-add-variable = Add Variable
env-new = New Environment
env-delete-confirm = Delete environment "{ $name }"?

## Connections editor
conn-name = Name
conn-kind = Kind
conn-folder = Folder
conn-tags = Tags
conn-environment = Environment
conn-read-only = Read-Only
conn-fields = Kind-Specific Fields (TOML)
conn-new = New Connection
conn-delete-confirm = Delete connection "{ $name }"?
conn-none = No connections yet.
conn-toml-error = The fields are not valid TOML: { $error }

## Vault dialogs
vault-create-title = Create Your Secret Vault
vault-create-body = Choose a master password. It protects every password, token and key HyperStack stores. There is no recovery if you forget it.
vault-unlock-title = Unlock the Secret Vault
vault-password = Master Password
vault-password-confirm = Confirm Master Password
vault-mismatch = The passwords do not match.
vault-wrong = Wrong master password.
vault-unlock = Unlock
vault-create = Create Vault
vault-later = Later
vault-locked-toast = The vault is locked.
vault-unlocked-toast = The vault is unlocked.

## Tools (built-in)
tool-terminal = Terminal
terminal-profiles = Shells
terminal-profile = Shell
terminal-restart = Restart
terminal-clear = Clear
terminal-exited = The shell exited with code { $code }.
terminal-no-shell = No shell was found on this machine.
terminal-hint = Click a shell to open it in a new tab. Select text and right-click to copy it; right-click again to paste. Ctrl+Shift+C and Ctrl+Shift+V also work. Shift+Page Up scrolls back.
settings-terminal = Terminal
settings-terminal-default-profile = Default Shell
settings-terminal-font-size = Font Size
settings-terminal-scrollback = Scrollback Lines
settings-terminal-copy-on-select = Copy on Select
settings-terminal-right-click = Right Click Copies and Pastes
settings-terminal-right-click-hint = Right-click copies the selected text; right-click with nothing selected pastes the clipboard (PuTTY style).
tool-scratchpad = Scratchpad
tool-scratchpad-hint = Markdown notes for this workspace. Saved with the layout.
tool-hello = Welcome
tool-hello-body = HyperStack is running. Press Ctrl+T for a new tab and Ctrl+Shift+P for commands.
tool-hello-toast-button = Toast Test
table-copy-row = Copy Row

## Commands (palette)
cmd-new-tab = New Tab
cmd-close-tab = Close Tab
cmd-reopen-tab = Reopen Closed Tab
cmd-next-tab = Next Tab
cmd-prev-tab = Previous Tab
cmd-split-right = Split Right
cmd-split-down = Split Down
cmd-toggle-sidebar = Toggle Sidebar
cmd-toggle-dock = Toggle Bottom Dock
cmd-settings = Open Settings
cmd-shortcuts = Keyboard Shortcuts
cmd-lock-vault = Lock Vault
cmd-unlock-vault = Unlock Vault
cmd-save-layout = Save Layout
cmd-zoom-in = Zoom In
cmd-zoom-out = Zoom Out
cmd-open-tool = Open { $tool }
cmd-switch-env = Switch to Environment: { $name }
cmd-switch-workspace = Switch to Workspace: { $name }

## Toasts
toast-saved = Saved.
toast-layout-saved = Layout saved.
toast-settings-saved = Settings saved.
