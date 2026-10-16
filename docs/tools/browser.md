# Tool spec — Browser

**Crate:** `tool-browser` (chrome) + `hyperstack::browser_host` (native views) · **Priority:** P1 (owner's request, 2026-10-15) · **Status:** implemented on Windows (WebView2); other platforms build the tool without a page area

## 1. Purpose

A small, secured browser inside the workspace: look at a deployed web app
or its docs without leaving HyperStack, open links from the terminal, and
keep the pages with the workspace. It is deliberately not a general-purpose
browser: no bookmarks sync, no extensions, no profiles. What it has is what
a developer needs next to the tools: tabs, an address bar, an ad and tracker
blocker, a JavaScript switch, forced dark rendering, zoom, developer tools
and print.

## 2. Layout

```
┌ page tabs (multi-row, same strip as the editor's file tabs) ───────── + ┐
│ ◀ ▶ ⟳ ⌂ [ https://…                                   ] → │ JS 🛡 ☾ 🔍- 🔍+ 🐞 🖨 │
│ Loading… · Blocked 3: https://ads.example.net/…                       │
│ ┌─────────────────────────────────────────────────────────────────┐ │
│ │                        native web view                           │ │
│ └─────────────────────────────────────────────────────────────────┘ │
```

- **Page tabs** use `hs_ui::tabs::TabStrip` in compact mode, so they wrap
  onto more rows like every other strip; close, close others, duplicate,
  copy URL in the context menu; drag to reorder.
- **Address bar**: Enter navigates; text without a scheme becomes `https://`
  when it looks like a host, otherwise a DuckDuckGo search.
- **Per-page switches** (each page remembers its own): JavaScript (off
  rebuilds the page without scripts), ad blocker, forced dark.
- **Zoom** in and out, **Inspect** (WebView2 developer tools in their own
  window), **Print** (the page's own print dialog, to paper or PDF).
- A status line shows loading, the number of blocked navigations and the
  last blocked URL, and errors from the host.

## 3. How the page area works

An egui application cannot draw HTML, so the page is a **native child web
view** (`wry` over WebView2) owned by the shell, not by the tool:

1. Each frame the tool reports, through `tool_browser::SharedBrowser`,
   the rectangle where its active page should appear, the list of pages
   with their switches, and commands (navigate, back, forward, reload,
   stop, devtools, print, clear data).
2. After the tools drew, `hyperstack::browser_host::BrowserHost::sync`
   creates a view per page on first use, moves it to the rectangle
   (physical pixels from egui points × `pixels_per_point`), shows the
   active page and hides the rest, rebuilds a page whose JavaScript
   switch changed, injects the dark/ad styles, and drops views whose page
   closed.
3. The host hides every view while a modal, the palette or the
   notification popup is open, because a native window always paints over
   egui. Menus that drop over the page area are the known exception.
4. Events flow back the same way: document title, URL and loading state
   per page, blocked navigations, requests for a new window (opened as a
   new page), errors.

The `wry` dependency is Windows-only in `crates/hyperstack/Cargo.toml`;
Linux and macOS compile the tool and show "the page area needs a native
web view" until those targets get one (webkit2gtk / WKWebView come with
`wry` too, but are untested here).

## 4. Blocking

- **Navigation blocklist**: a bundled list of ad and tracker hosts
  (`tool_browser::DEFAULT_BLOCKLIST`, about sixty domains) plus
  Settings → Browser → Extra Blocked Hosts. A navigation to a listed host
  or any of its subdomains is refused by the view's navigation handler and
  counted on the page's status line.
- **Cosmetic hiding**: an initialization script runs at document start on
  every page and defines `window.__hs.ads(on)`, which hides the usual ad
  containers (`.adsbygoogle`, `[id^="div-gpt-ad"]`, Taboola/Outbrain
  widgets, third-party ad iframes) by style. The host cannot filter
  subresource requests, so an ad that loads is hidden rather than never
  fetched; the blocklist above stops the top-level trackers.
- **JavaScript off** is a WebView2 setting fixed at creation, so flipping
  it rebuilds the page.
- **Forced dark**: `window.__hs.dark(on)` inverts the page with a hue
  rotation and inverts pictures back, and sets `color-scheme: dark`.

## 5. Links from other tools

`OpenRequest::Url { url }` opens a page in the existing browser tab (or a
new one). The terminal and the dock terminal send it on **Shift+click** of
a URL; a plain click still opens the system browser. Home → Browser and
the launcher open the home page from Settings → Browser.

## 6. Persistence and privacy

Pages (URL, title, switches, zoom) are saved in the tab's layout state and
restored with the workspace. WebView2 keeps its own cookies and cache in
the user data folder it chooses (WebView2's default profile folder under
the executable's name); **Clear Browsing Data** empties it. Nothing about
browsing is written to `HyperStackData/` except the page list in
`layout.json`.

## 7. Settings → Browser

Home page, default ad blocker / JavaScript / forced-dark state for new
pages, extra blocked hosts (one per line).

## 8. Tests

`tool_browser::is_blocked` (hosts, subdomains, extras, non-URLs) and the
address normaliser have unit tests. The host is exercised in the running
app (docs/screenshots: `browser.png`).

## Addendum (2026-10-16): Inspect docked under the page

WebView2 has no way to dock its developer tools: `OpenDevToolsWindow` opens
a window and that is the whole API. The docked panel is therefore built on
the Chrome DevTools Protocol, which the web view speaks when it is started
with `--remote-debugging-port`:

- Settings → Browser → **Dock Inspect Under the Page** turns it on. The
  port is chosen when the setting is switched on (a free one, bound to
  127.0.0.1), and the open pages are rebuilt, because the arguments a web
  view starts with cannot be changed afterwards.
- Inspect then splits the tab: the page keeps the top, and a second web
  view under it loads the DevTools front end that the web view itself
  serves on that port (`/devtools/inspector.html?ws=…`). Nothing is
  fetched from the network for it.
- `--remote-allow-origins` names the front end's own origin. Without it
  Chromium refuses the WebSocket and the panel shows "Debugging connection
  was closed".
- The inspector's own navigation is not reported as the page's, so the
  address bar keeps showing the page.
- The page to inspect is matched by its URL. Two tabs on the same URL may
  attach the inspector to the other one.

**The cost, and why it is off by default.** While the setting is on, every
web view in the session listens for the DevTools protocol on that local
port. Any program running as this user can then drive the browser tabs:
read their cookies, navigate them, read page content. That is why the
setting is off until it is asked for, and why Inspect opens WebView2's own
window otherwise.
