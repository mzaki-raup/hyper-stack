# Tool spec — IDM (Identity Management)

**Crate:** `tool-idm` · **Status:** foundation built (provider, Keycloak, safety layer); screens pending · **Last updated:** 2026-11-08 ·
**Phase:** P of [docs/10-depth-programme.md](../10-depth-programme.md) ·
**Ledger:** `FEAT-0109` to `FEAT-0116` (task version `v0.8.0`) ·
**Decisions:** [ADR-032](../08-decisions.md)

## 1. Purpose

Administer the identity servers a team runs from inside the workspace:
find a user, see why they cannot sign in, put them in the right group,
give or take a role, reset a password, end their sessions, look at a
client's settings and its secret, read the login and admin events - the
everyday work of an identity server's admin console, through the
server's own admin API.

The owner's words (2026-11-08): *"add IDM as new Activity… For starter,
Keycloak. If provide keycloak admin credentials, can the work pane
resemble keycloak web interface (using APIs, etc)?"* - so the work pane
follows the shape of Keycloak's admin console (realm picker, a section
column, a list, an item page with its tabs), drawn with HyperStack's own
components and rules ([.claude/rules/ui.md](../../.claude/rules/ui.md)).

The activity is called **Identity** on the rail, in menus and in the
palette (owner, 2026-11-08; Fluent `tool-idm = Identity`); *IDM* stays
the crate's and this spec's name.

One activity serves every identity product through a provider trait
(§4.1, ADR-032). **Keycloak is the first provider**; **LDAP and Active
Directory** are the next (§4.12, owner, 2026-11-08); Authentik, Zitadel,
Okta and Microsoft Entra ID slot in later behind the same trait and the
same screens. Apereo CAS is on hold (§11).

It is **not**:

- a replacement for every page of the admin console. Themes, the account
  console, impersonation and anything §4.11 leaves out stay in the
  server's own UI, reached with **Open in Admin Console** through the
  Browser activity (`OpenRequest::Url`).
- an identity provider itself, a login page or an SSO broker for
  HyperStack.
- a bulk user import tool (no CSV import of users in any phase here).

## 2. User stories

- As a **support engineer**, I want to type part of a name or an email,
  find the user among 200 000 without the app loading them all, and see
  that the account is disabled, has *Update Password* pending and was
  last locked out at 09:12, so I can answer the ticket.
- As a **support engineer**, I want to reset a user's password as
  temporary and log out their sessions, behind a confirmation, and have
  a record of what I changed.
- As a **full-stack developer**, I want to see which realm and client
  roles a test user holds - directly and through groups - and add the
  one my feature checks, so I can test the permission without asking an
  admin.
- As a **DevOps / SRE**, I want to read a client's redirect URIs, web
  origins and flow switches, view its secret masked and copy it when I
  mean to, and regenerate it with a typed confirmation when it leaked.
- As a **DevOps / SRE**, I want the failed logins of the last hour with
  their error and IP address, and the admin events that changed a
  client, so I can tell an attack from a broken deploy.
- As a **tester**, I want production realms to open read-only, so a
  click meant for staging cannot disable a real user.
- As anyone, I want a realm's clients, groups and roles as one JSON
  document, read-only, to read or diff in the Editor.

## 3. Layout

The connection-per-tab shape of the Tracker, Queue and Database
activities, with the inner layout of Keycloak's admin console.

### 3.1 Dashboard (the permanent home tab)

Through `hs_ui::dashboard`, with the same headings and top spacing as the
other activities:

- the band;
- summaries: *Servers* (saved), *Realms* (in the servers open in tabs),
  *Users* (the current realm's `/users/count` of each open tab, summed),
  *Open Tabs*. Figures come from open tabs only; a saved server is never
  connected to just to fill a number;
- **Start Here**: **Add Identity Server** (opens Settings → Identity with a new
  form, as Add Tracker does);
- **Saved Servers**: a card per server with its product's mark (§3.6),
  name, URL and default realm; a click opens or focuses its tab;
- **Recent Changes**: the last ten entries of HyperStack's own audit log
  (§6.4) - time, server, realm, verb, object - with a right-click menu
  carrying **Clear History** (broom, behind a confirmation, clears the
  file) and **Remove** for one row (CR-0067);
- **Open Here**.

### 3.2 Sidebar

`form::sidebar_head` (title, the labelled **Add Identity Server** button, the
rule), then the saved servers, one list: each row its product's mark,
name and URL, the grip to reorder (`hs_ui::reorder`). A click opens the
server's tab or focuses it - a server has at most one tab. Right-click:
**Connect**, **Edit This Connection**, **Open in Admin Console**,
**Delete** (trash can, confirmation; deletes the saved profile and its
vault entries, never anything on the server).

### 3.3 A tab per server

```
┌ header band: [Keycloak mark] Local Keycloak · http://127.0.0.1:8180 · 26.0.5 · signed in as hs-admin (hs-demo) ┐
│ [⇤ Browse] [⟳ Refresh] [↗ Open in Admin Console] [⤓ Export Realm]                              [✦ AI] │
├───────────────────┬──────────────────────────────────────┬────────────────────────────────────────────┤
│ Realm             │ [🔍 Search users          ] [⚙ Exact] │ [👤] alice                                 │
│ [hs-demo      ▾]  │ ┌ Username ─ Email ─ Name ─ Status ┐ │ ● Enabled  ● Email Verified  ● Temp. pwd   │
│ ───────────────── │ │ alice   alice@… Alice Li Enabled│ │ [Details|Attributes|Credentials|Groups|    │
│ 👤 Users          │ │ bob     bob@…   Bob Ng   Disabled│ │  Role Mapping|Sessions]                    │
│ 👥 Groups         │ │ …                                │ │ Username   alice                           │
│ 🛡 Realm Roles    │ └──────────────────────────────────┘ │ Email      [alice@example.com        ]     │
│ 🛡 Client Roles   │ 1–50 of 1,284   [‹ Previous] [Next ›] │ …                     [✓ Save] [⟲ Revert] │
│ ▣ Clients         │                                      │                                            │
│ ⏱ Sessions        │                                      │                                            │
│ ≡ Login Events    │                                      │                                            │
│ ≡ Admin Events    │                                      │                                            │
│ ⚙ Realm Settings  │                                      │                                            │
└───────────────────┴──────────────────────────────────────┴────────────────────────────────────────────┘
```

- **Header band** (`hs_core::tool::ToolHeader`): the product's mark and
  colour (the same picture as the tab icon and the sidebar row), name,
  URL, server version, and *signed in as* the client id or user with the
  realm it authenticated in. A version outside the supported range (§8.3)
  adds a warning chip.
- **Toolbar** (`hs_ui::toolbar`, main size): the **Browse** toggle that
  hides the left column is the left-most item (CR-0073); then
  **Refresh**, **Open in Admin Console**, **Export Realm**; the AI
  assistant is the hard-right item through `toolbar::hard_right`. The
  row wraps under a full-width rule on a narrow window. The section's
  own actions (Search, Save, Delete…) live in the list's and the
  detail's sub-toolbars (sub size), not here.
- **Left browse column** - resizable, hideable, keyboard-navigable:
  - **Realm**: a `form::pick_field` filled from `GET /admin/realms`; a
    realm typed that the list lacks is tried and reported if refused.
    Switching realm drops the section's list, selection and caches.
  - the **sections**, an item-kind column (20 px icon, body text, 30 px
    rows, each kind its own colour; Material pictures when on; CR-0074):
    Users, Groups, Realm Roles, Client Roles, Clients, Sessions, Login
    Events, Admin Events, Realm Settings. A section the server or the
    account cannot offer (§8.3) is drawn disabled with a tooltip saying
    why, never left out.
- **Middle: the section's list** - a sub-toolbar (search field, filters,
  the section's verbs), `PANE_GAP`, a `hs_ui::table::DataTable` (header
  band, indented titles, centred cells, standard row heights), `PANE_GAP`,
  and the paging status line (*1–50 of 1,284*, **Previous** / **Next**,
  or **More** where the server gives no count). Empty, loading and error
  states through the shared helpers ("No users match *ali*.").
- **Right: the item page** of the clicked row - a structured header (the
  item's kind picture, its name, chips for its state), a segmented tab
  strip of the item's pages (the pane cannot be dragged narrower than
  `hs_ui::tabs::segmented_min_width` of that strip, CR-0069), then the
  page inside `form::pane`. Forms through `form::field_grid`, facts
  through `form::detail_table`, editable rows through `table::edit_table`,
  name = value pairs through `hs_ui::kv_editor::KvEditor`. With nothing
  selected the column says so ("Select a user to see it here.").
- `hs_ui::env_banner` across the tab and the 2 px frame when the
  profile's environment is protected; the tab is then read-only (§6.1).

### 3.4 Settings → Identity

Between Tracker and the following page, list left and form right like
Settings → Tracker (CR-0044): one **Add Identity Server** button, the
provider as a dropdown (only Keycloak in phase 1), and the form of §5.1
as a `form::field_grid` with `form::label` / `form::sub_label`, buttons
through `form::button` / `form::primary_button`, **Test Connection**
(token request plus `serverinfo`, reported in the form), **Save**,
**Delete**. Below the list, the activity's defaults (§7: page size,
request timeout).

### 3.5 Every screen keeps the layout standard

Every screen in §4 is checked against the *Checking a screen* list of
`ui.md` before its item is reported: fields fit their values, one
height and one middle line, sections set apart with `PANE_GAP` /
`ROW_GAP`, `PANE_MARGIN` in from every edge, the component table, table
headers and heights, Title Case labels and sentence-case hints from
Fluent, the three states, icon + label on every button and wrapping
rows. Column floors add up to no more than the usual work pane: with
the browse and detail columns open, the Users list shows Username
(200), Email (220), Name (180) and Status (90), and Status gives way
first on a narrow pane.

### 3.6 Marks and colours

- **Keycloak** wears its real multi-colour logo everywhere it is drawn -
  the sidebar row, the tab, the header band, the dashboard card, Home's
  Recent, Settings → Identity (CR-0079). Devicon (v2.18.0 and its latest)
  has no Keycloak icon, so the mark is **Simple Icons' `keycloak` path,
  recoloured** (owner's decision, 2026-11-08): the outer shape in
  Keycloak's blue `#008AAA`, the two brackets in white, so it reads on
  both themes (Simple Icons' own `#4D4D4D` is a hole on the dark one).
  It is `assets/icons/simple/keycloak.svg`, registered in
  `hs_ui::icons::PRODUCT_LOGOS` as `bytes://product/keycloak.svg` and
  mapped from the `keycloak` kind; its line is in the icon folder's
  README. Where no logo can be drawn the glyph is Phosphor's
  `SHIELD_CHECK`.
- The sections each wear their own colour and picture (users, groups,
  roles, clients, sessions, events, settings), the same in the browse
  column, the item page's header and any tab or palette row.
- Several saved servers of the same product are told apart by the
  rotating palette of eight, by position (CR-0059).

## 4. The sections

### 4.1 The provider model

```rust
#[async_trait]
pub trait IdmProvider: Send + Sync {
    async fn connect(&self) -> Result<ServerInfo>;           // token + server facts
    fn capabilities(&self) -> &Caps;                          // from ServerInfo (§8.3)
    async fn realms(&self) -> Result<Vec<Realm>>;
    async fn realm(&self, realm: &str) -> Result<RealmSummary>;
    // users
    async fn users(&self, realm: &str, q: &UserQuery, page: Page) -> Result<Paged<UserRow>>;
    async fn user_count(&self, realm: &str, q: &UserQuery) -> Result<Option<u64>>;
    async fn user(&self, realm: &str, id: &str) -> Result<User>;
    async fn update_user(&self, realm: &str, id: &str, change: &UserChange) -> Result<()>;
    async fn reset_password(&self, realm: &str, id: &str, pw: SecretString, temporary: bool) -> Result<()>;
    async fn delete_user(&self, realm: &str, id: &str) -> Result<()>;
    // groups, roles, clients, sessions, events, export: the same shape
    // (list with a Page, one item, the writes of §4.3-§4.10)
    async fn export(&self, realm: &str, what: ExportScope, cancel: CancelToken) -> Result<String>;
}
```

- The trait speaks the **shared concepts**: realm (Keycloak's realm,
  Authentik's tenant/brand, Zitadel's organisation, Entra's tenant), user,
  group, role (realm or client), client/application, session, event.
  Names that differ per product are the provider's labels, not new
  screens.
- `Caps` says what this server and this account can do - which sections
  exist, which writes are allowed, which fields the user form shows.
  Screens ask `Caps`, never the provider's name, so a later provider
  lights up only what it has.
- Paging is one type, `Page { first, max }` and `Paged<T> { rows, total:
  Option<u64>, more: bool }`, so a server that counts and one that only
  says "there is more" draw the same status line.
- A `Guarded` wrapper (the shape of `tool-queue::driver::Guarded`) sits
  between the UI and the provider: on a read-only tab every write method
  returns the refusal before any request is made (§6.1).
- Keycloak is `provider/keycloak/` (§8.1). Each provider is one module;
  no screen imports one.

### 4.2 Connecting, the realm and Realm Settings

- Opening a tab runs `connect`: the token (§5.2), then
  `GET /admin/serverinfo` for the version, the enabled features and the
  providers; then `GET /admin/realms?briefRepresentation=true` for the
  realm picker. The tab opens on the profile's **default realm**, else the
  realm it authenticated in.
- **Realm Settings** (phase 1, read-only): `GET /admin/realms/{realm}`
  shown as `form::detail_table` groups - General (name, display name,
  enabled, SSL required, frontend URL), Login (registration, remember me,
  login with email, verify email, reset password allowed, edit username
  allowed), Tokens (access token lifespan, SSO idle / max, offline
  idle), Security (brute-force detection on, failure factor), Events
  (from `GET /admin/realms/{realm}/events/config`: user events on and
  their expiration, admin events on and with details). **Open in Admin
  Console** goes to the realm's settings page there.

### 4.3 Users

**List** - `GET /admin/realms/{realm}/users?search=&first=&max=&briefRepresentation=true`

- The search field searches username, email, first and last name
  (Keycloak's `search`, which matches by prefix); **Exact** sends the
  term as `username=` (or `email=` when it holds an `@`) with
  `exact=true` - Keycloak ignores `exact` beside `search`, as the bench
  showed (`search=alice&exact=true` still finds `alice.nguyen`); an
  `attr:value` term becomes the `q=attr:value` attribute search; an
  **Enabled** dropdown (Any / Enabled / Disabled) sends `enabled=`.
  Typing is debounced 300 ms and each request carries a generation, so
  a late answer for an older query is dropped.
- The total comes from `GET /admin/realms/{realm}/users/count` with the
  same filters; the list is never fetched whole.
- Columns: Username, Email, Name (first and last), Status (Enabled /
  Disabled chip; a *Temporarily Locked* chip when brute-force detection
  reports it for the selected user).
- Sub-toolbar verbs on the selected row: **Enable** / **Disable**,
  **Reset Password…**, **Log Out**, **Delete…**, **Open in Admin
  Console**.

**Item page** - `GET /admin/realms/{realm}/users/{id}` (with
`userProfileMetadata=true` on 24+), header chips Enabled, Email
Verified, Required Actions pending, Federation link (read-only when the
user comes from LDAP or another federation provider).

| Tab | Shows | Does | Endpoints |
|---|---|---|---|
| **Details** | id, created, username, email, email verified, first / last name, enabled, required actions | edit the fields; Required Actions is a `pick_field` list of the realm's enabled actions (`GET /authentication/required-actions`) one per pick | `PUT /users/{id}` |
| **Attributes** | the user's attributes | edit (§4.3.1) | `PUT /users/{id}` |
| **Credentials** | the credential types the user has (password, OTP, WebAuthn) with their label and created date - **metadata only**, the server never returns secrets | **Reset Password…** | `GET /users/{id}/credentials`, `PUT /users/{id}/reset-password` |
| **Groups** | the user's groups (path), paged | **Join Group…** (a group picker searching the tree), **Leave** per row | `GET /users/{id}/groups?first&max&briefRepresentation=true`, `PUT` / `DELETE /users/{id}/groups/{groupId}` |
| **Role Mapping** | assigned realm and client roles, with **Show Effective** adding inherited ones (from composites and groups) marked as such | **Assign Role…** (available realm roles, or a client then its available roles), **Unassign** per assigned row; effective rows cannot be unassigned here and say where they come from | `GET /users/{id}/role-mappings`, `…/realm`, `…/realm/available`, `…/realm/composite`, `…/clients/{client}`, `…/clients/{client}/available`, `…/clients/{client}/composite`, `POST` / `DELETE` on `…/realm` and `…/clients/{client}` |
| **Sessions** | active sessions (IP, started, last access, clients) and offline sessions per client | **Log Out** (all of the user's sessions), **End Session** per row | `GET /users/{id}/sessions`, `GET /users/{id}/offline-sessions/{client}`, `POST /users/{id}/logout`, `DELETE /admin/realms/{realm}/sessions/{session}` |

Rules for the writes:

- **Saving the Details or Attributes page** reads the user again, merges
  the edited fields into the **whole** representation and `PUT`s that.
  A partial body is never sent: on 24+ the declarative user profile
  validates the whole user, and attributes left out of a `PUT` are
  removed. Before the `PUT`, a review lists each changed field with its
  old and new value (§6.2); if the server's copy changed since the page
  was read (another admin), the review says so and offers Reload.
- **Enable / Disable** is the same read-merge-write of `enabled`, with a
  confirmation naming the user.
- **Reset Password…** opens a modal: `form::secret_field` twice (typed
  and confirmed), **Temporary** checked by default (the user must change
  it at next login), the realm's password policy shown as a hint. The
  value goes straight from the field to the request body as
  `{ type: "password", value, temporary }`, is zeroised after the send,
  and is never stored, logged, put in history, shown in a toast or given
  to the assistant. A policy refusal (400) is shown in the modal.
- **Delete…** is a typed confirmation: the username must be typed. A
  federated user's deletion says it removes the local link only where
  the provider is read-only.
- **Log Out** ends every session of the user, with a confirmation.

#### 4.3.1 Attributes and the user profile

- **Keycloak 24+** (declarative user profile always on): the Details
  page draws the profile's attributes from `userProfileMetadata` - each
  with its display name, required flag, read-only flag (`readOnly` for
  admins), and validators shown as hints - in the profile's groups and
  order. The Attributes tab shows only **unmanaged** attributes, editable
  when the realm's `unmanagedAttributePolicy` is `ENABLED` or
  `ADMIN_EDIT`, read-only for `ADMIN_VIEW`, and hidden (with a sentence
  saying so) when unset.
- **Keycloak 22-23**: attributes are a free map in a `KvEditor` (values
  are lists; one row per value), unless the realm enabled the
  declarative-user-profile preview, which is then treated as 24+.

### 4.4 Groups

- **Tree** (`hs_ui::tree`, lazy): top-level groups from
  `GET /groups?first&max&search&briefRepresentation=true`, counted by
  `GET /groups/count`; children on expand from
  `GET /groups/{id}/children?first&max` (23+) or from the parent's
  `subGroups` (22). A node shows its `subGroupCount` where the server
  gives it; **More** loads the next page of a level. Search shows the
  matching groups with their paths.
- **Item page**: header with the group's path.
  - **Members**: `GET /groups/{id}/members?first&max&briefRepresentation=true`,
    paged; **Include Sub-Group Users** off by default (it walks the
    subtree: a Job with progress and cancel); **Add Member…** (a user
    search) and **Remove** per row, through the user's group endpoints.
  - **Role Mapping**: as the user's, on `/groups/{id}/role-mappings/…`.
  - **Attributes**: `KvEditor`, saved by read-merge-write of
    `PUT /groups/{id}` with the review.
  - **Details**: name and path; **Rename** (read-merge-write).
- **Delete…** (typed confirmation: the group's name) says how many
  sub-groups go with it, from `subGroupCount`.
  `DELETE /groups/{id}`.

### 4.5 Realm Roles and Client Roles

- **Realm Roles**: `GET /roles?first&max&search&briefRepresentation=true`,
  paged with **More** (the server gives no count). Columns: Name,
  Composite (yes / no), Description.
- **Client Roles**: a client picker (`pick_field` over the realm's
  clients, searched), then `GET /clients/{client}/roles?first&max&search`.
- **Item page**:
  - **Details**: name (read-only), description (edit,
    read-merge-write of `PUT /roles-by-id/{id}` with the review);
  - **Composite Roles**: read-only in phase 1, from
    `GET /roles-by-id/{id}/composites`, realm and client roles marked;
  - **Users in Role**: `GET /roles/{name}/users?first&max` (realm) or
    `GET /clients/{client}/roles/{name}/users?first&max`, paged;
  - **Groups in Role** where the server offers it
    (`…/roles/{name}/groups`).
- **Delete…** (typed confirmation: the role's name) warns when the role
  is a default role or part of a composite. `DELETE /roles-by-id/{id}`.
  Built-in roles (`offline_access`, `uma_authorization`,
  `default-roles-{realm}`, the `realm-management` client's roles) are
  never offered for deletion.

### 4.6 Clients

- **List**: `GET /clients?first&max&search=true&clientId=` (the search
  matches the client id), paged with **More**. Columns: Client ID, Name,
  Type (OpenID Connect / SAML), Access (Confidential / Public / Bearer
  Only), Enabled.
- **Item page**:
  - **Settings** - the common fields in a `field_grid`, edited by
    read-merge-write of `PUT /clients/{id}` with the review: name,
    description, enabled, root URL, home URL, valid redirect URIs, valid
    post-logout redirect URIs, web origins (each a one-per-row list),
    admin URL; client authentication (confidential), standard flow,
    implicit flow, direct access grants, service accounts, OAuth 2.0
    device grant, front-channel logout, consent required. Fields beyond
    these are listed read-only in a **More Settings** `detail_table`, and
    the page offers **Open in Admin Console** for them. SAML clients get
    the common fields only; their SAML settings are read-only.
  - **Credentials** (confidential clients): the client authenticator, and
    the secret from `GET /clients/{id}/client-secret`, **masked** as
    `••••••••` with **Show** (for 30 s, or until the page is left) and
    **Copy** (an explicit click; the clipboard is cleared after 60 s,
    as the Security page does). **Regenerate Secret…** is a typed
    confirmation (the client id) that says every service using the old
    secret stops working: `POST /clients/{id}/client-secret`; the new
    secret is shown masked like the old one.
  - **Roles**: the client's roles, as §4.5's Client Roles for this
    client.
  - **Service Account Roles** (service accounts on): the role mappings
    of `GET /clients/{id}/service-account-user`, edited as a user's.
  - **Sessions**: `GET /clients/{id}/user-sessions?first&max` and
    `GET /clients/{id}/session-count`.
- **Delete…** (typed confirmation: the client id). `DELETE /clients/{id}`.
  The realm's own clients (`account`, `account-console`, `admin-cli`,
  `broker`, `realm-management`, `security-admin-console`, and in master
  the `{realm}-realm` clients) are never offered for deletion, and their
  settings open read-only.

### 4.7 Sessions

- **Realm sessions**: the realm's clients with their active and offline
  counts from `GET /client-session-stats`; selecting a client lists its
  sessions (`GET /clients/{id}/user-sessions?first&max`) - user, IP,
  started, last access - with **End Session** per row
  (`DELETE /sessions/{session}`). The admin console's own
  `ui-ext` session endpoints are not used: they are internal and change
  between releases.
- **Log Out All Sessions…** (typed confirmation: the realm's name):
  `POST /admin/realms/{realm}/logout-all`; the answer's count of clients
  it could not reach is shown.

### 4.8 Events

- **Login Events**: `GET /events?type=&client=&user=&ipAddress=&dateFrom=&dateTo=&first&max`.
  Filters: event type (a multi-pick of the server's types from
  `serverinfo`), client, user (id or username, resolved), IP, a date
  range (`hs_ui::datefield::date_field`, `YYYY-MM-DD`). Columns: Time,
  Level, Type, User, Client, IP, Error. **Level** is the word `ERROR`
  for a type ending in `_ERROR` (or with an `error` set) and `INFO`
  otherwise, coloured through the shared log-level function
  (`hs_ui::loglevel`); only that word is coloured, the rest of the row
  keeps the normal text colour (CR-0077). The detail shows the event's
  `details` map in a `detail_table`.
- **Admin Events**: `GET /admin-events?operationTypes=&resourceTypes=&resourcePath=&authRealm=&authClient=&authUser=&authIpAddress=&dateFrom=&dateTo=&first&max`.
  Columns: Time, Operation (CREATE / UPDATE / DELETE / ACTION), Resource
  Type, Resource Path, By (user or client), IP. The detail shows the
  representation the event carries (when the realm stores details) in a
  read-only JSON view, with secret-looking fields masked (§6.5).
- Both are paged with **More**, newest first. When the realm does not
  store that kind of event (`events/config`), the table's empty state
  says so - "This realm does not store login events." - with **Open in
  Admin Console** to turn it on; changing event settings is not offered
  in phase 1.

### 4.9 Realm export

- **Export Realm** (toolbar and palette): a modal with **Include Clients**
  and **Include Groups and Roles** (both on), then
  `POST /admin/realms/{realm}/partial-export?exportClients=&exportGroupsAndRoles=`
  as a Job with cancel.
- The JSON opens **read-only** in an inner view drawn with the editor
  engine (`hs-editor`, read-only, folding, search), with **Save As…** (a
  file the user chooses), **Open in Editor** (`OpenRequest::Text` with
  `application/json`), and **Copy**. Users are not part of a partial
  export; the view says so.
- Keycloak masks client secrets in a partial export (`**********`); the
  view checks for any unmasked `secret` / `value` / `password` field and
  masks it before showing it (§6.5).
- Import is not in phase 1 (§4.11).

### 4.10 What every page has

- **Refresh** (F5) re-reads the list and the open item.
- Every write ends with a toast where the user started it, an entry in
  the audit log (§6.4) and a re-read of what it changed.
- A 404 on an item that was just listed says it is gone and drops it
  from the list.
- **Create** (owner, 2026-11-08: phase 1, not phase 2): Users → **Add
  User…** (username, email, first and last name, enabled, email
  verified, required actions; `POST /users`), Groups → **Create Group…**
  and **Create Sub-Group…** (`POST /groups`, `POST
  /groups/{id}/children`), Realm Roles and Client Roles → **Create
  Role…** (name, description; `POST /roles`, `POST
  /clients/{client}/roles`), Clients → **Create Client…** (client id,
  name, description, protocol, client authentication, service accounts,
  standard flow, direct access grants, redirect URIs, web origins;
  `POST /clients`). Each is a short modal form, guarded and audited like
  every write; the new item is selected and its page opened.

### 4.11 Later phases (specified, not built now)

**Phase 2**

- **Identity Providers** and their **mappers**: list, view, edit the
  common fields, enable / disable; secrets masked as in §4.6.
- **Client Scopes** and **Protocol Mappers**: list, view, edit; a
  client's assigned default / optional scopes.
- **Partial Import** with a **diff preview**: a JSON file is compared
  with the realm (New / Differs / Same per object), the user picks the
  policy (skip / overwrite / fail) per kind, and
  `POST /partial-import` runs only after the preview, guarded, with a
  typed confirmation on overwrite.
- **Authentication flows**, read-only: the flows, their executions and
  requirements as an indented tree.
- **Realm compare**: two realms (same or different servers) compared by
  clients, roles, groups and realm settings, as the Database schema
  compare does.
- Execute Actions Email; Unlock User (brute-force); delete a user's
  credential; event settings. (Creating users, groups, roles and clients
  moved into phase 1, §4.10.)

**Phase 3**

- Authentication **flow editor**.
- **Authorization services** (resources, scopes, policies, permissions)
  and **fine-grained admin permissions** (v1, and v2 on 26.2+).
- **User federation** (LDAP / Kerberos providers: view, sync, test).
- **Organizations** (25 preview, 26 supported): list, members, identity
  providers, domains.

**Out of scope**: themes, the account console, impersonation, realm
creation and deletion, server-wide settings, key providers (read-only
listing is phase 2 at most). These are reached through **Open in Admin
Console**.

**Later providers**: Authentik (API v3, token), Zitadel (Management and
Admin APIs, service user JWT or PAT), Okta (Management API, API token or
OAuth service app), Microsoft Entra ID (Microsoft Graph, client
credentials - REST, not LDAP). Each brings its own `Caps`; none changes a
screen's shape. LDAP and Active Directory come first of them (§4.12).

### 4.12 The LDAP and Active Directory provider (next after Keycloak)

Owner, 2026-11-08: "add LDAP and AD in the list". One provider speaks
LDAP v3 to any directory - OpenLDAP, 389 Directory Server / FreeIPA,
OpenDJ, Samba AD and Microsoft AD DS; an **Active Directory** mode adds
what only AD has. The directory is where the users live, so the user and
group screens are the same pages Keycloak fills; the rest are drawn
disabled with the reason, through `Caps`.

| Section | LDAP | Active Directory mode |
|---|---|---|
| Realm switcher | the naming contexts (base DNs) from the Root DSE | the domain's default naming context |
| Users | search by `uid` / `cn` / `mail` under a configurable users base and filter; view, create, edit attributes, delete, rename / move (`modrdn`) | `sAMAccountName` / `userPrincipalName`; enable / disable (`userAccountControl`), unlock (`lockoutTime`), *must change password at next logon* (`pwdLastSet = 0`), account expiry |
| Password reset | `userPassword` (the Password Modify extended operation where the server has it) | `unicodePwd`, only over LDAPS or StartTLS; refused in words on a plain connection |
| Groups | `groupOfNames` / `groupOfUniqueNames` / `posixGroup`; members, add / remove, create, delete | security and distribution groups, scope, nested groups and `memberOf` (effective membership through the in-chain matching rule) |
| **Directory** (new section) | the DIT as a tree: OUs and containers, any entry's attributes in a raw editor with the schema's attribute types, a review before each save (`Change::review`) | the same, with the domain's OUs, Users and Computers |
| Roles, Clients, Sessions, Events | disabled: "A directory has no clients, sessions or events." | disabled; AD's sign-in events are in the Windows event log, not LDAP |

- **Connection** (`[idm]` with kind `ldap` or `active-directory`):
  URL `ldap://` or `ldaps://`, a StartTLS switch, Verify TLS and CA File
  as §5.1, bind DN (or UPN for AD) with the password as `secret:<ulid>`,
  users base / groups base / user filter (defaults per mode), page size.
  An anonymous bind is read-only. Windows integrated (Kerberos / GSSAPI)
  login is a later step, not this one.
- **Paging**: the Simple Paged Results control (AD's `MaxPageSize` is
  1000); Server Side Sort where offered, else sorted per page.
- **Guards**: the same `Guarded` wrapper, protected-environment
  read-only, typed confirmations for delete and for moving an entry with
  children, the workspace audit log (DNs and attribute names, never
  values). A password is never read back - LDAP returns only hashes, and
  they are masked like any secret.
- **Dependency**: an async LDAP client (`ldap3` on tokio) - its own ADR
  and a `cargo deny` pass before it is added.
- **Bench**: an OpenLDAP container and a Samba AD domain controller in
  the bench kit's `idm` profile, seeded with an OU tree, users and nested
  groups; live tests gated on `HS_LDAP` and `HS_AD`.

## 5. Connections and credentials

### 5.1 The profile

Connection kind `keycloak` in `hs_core::connections` (later kinds
`authentik`, `zitadel`, `okta`, `entra-id`, `ldap`; `connections::IDM_KINDS`
and `is_idm_kind` list them, as `is_tracker_kind` does). The fields sit in
the profile's `[idm]` table beside the credential's flat keys
(`hs_core::service_auth::Credential`, ADR-028):

```toml
kind = "keycloak"
name = "Local Keycloak"
environment = "dev"

[idm]
url = "http://127.0.0.1:8180"     # base URL, no /admin; include /auth for an old WildFly-style path
auth_realm = "master"              # the realm the token is requested from
default_realm = "hs-demo"          # the realm the tab opens on; empty = auth_realm
client_id = "hs-admin"             # admin-cli for the password kind
tls_verify = true
ca_file = ""                       # a PEM file of extra roots (a company CA); empty = system roots only
auth = "api-key"                   # api-key | password | token
user = ""                          # the admin's username for the password kind
secret = "secret:01JC…"            # always a vault reference once saved
```

| Field | Component | Notes |
|---|---|---|
| Name | `text_field` | |
| Provider | dropdown | Keycloak (only, in phase 1) |
| Base URL | `text_field` | validated as an http(s) URL; hint: "Keycloak 17+ has no /auth prefix" |
| Authentication | dropdown | Client Credentials · Admin Password · Offline Token |
| Auth Realm | `pick_field` | default `master`; filled from the server once Test Connection passed |
| Client ID | `text_field` | Client Credentials: the confidential client; Admin Password: `admin-cli` by default |
| Client Secret / Password / Token | `form::secret_field` | stored as `secret:<ulid>` |
| Username | `text_field` | Admin Password only |
| Default Realm | `pick_field` | from the server after Test Connection |
| Verify TLS | checkbox | on by default; off draws the red *TLS not verified* badge in the header (docs/06 §5) |
| CA File | `text_field` with a file chooser | optional; a PEM bundle added to the system's roots for an internal Keycloak behind a company CA (owner, 2026-11-08); a missing or unreadable file is said in words at Test Connection |
| Environment | dropdown | the workspace's environments; decides protection (§6.1) |

The form shows only the fields of the chosen authentication. `{{var}}`
works in every text field through `hs_core::environments`.

### 5.2 Authentication and the token

| Authentication | `auth` kind | Grant | What the form asks |
|---|---|---|---|
| **Client Credentials** (preferred) | `api-key` | `client_credentials` with `client_id` + `client_secret` | client id, client secret |
| **Admin Password** | `password` | `password` against `admin-cli` (public), `username` + `password` | username, password (client id prefilled) |
| **Offline Token** | `token` | `refresh_token` with an offline token the user obtained elsewhere | client id, token |

The `api-key` kind carries the client secret in the request body, not in
a header, so its `header` key is unused and the form hides it.

- Tokens come from
  `POST {url}/realms/{auth_realm}/protocol/openid-connect/token`
  (`application/x-www-form-urlencoded`) and every admin call sends
  `Authorization: Bearer <access_token>`.
- Each tab's session holds the access token (and the refresh token the
  password and offline kinds get) **in memory only**, as
  `secrecy::SecretString`; nothing of a token is written to disk, the
  log, history or the assistant.
- **Refresh before expiry**: Keycloak's default access token lives 60 s
  (the realm's *Access Token Lifespan*), so the session renews when less
  than a third of `expires_in` (and at least 10 s) is left, before the
  next request rather than on a timer. Client credentials request a new
  token (Keycloak gives no refresh token for that grant by default); the
  password and offline kinds use `refresh_token`, and fall back to a new
  password grant when the refresh token itself expired (password kind
  only). One refresh at a time: concurrent requests wait on the same
  `tokio::sync::Mutex`.
- A **401** on an admin call refreshes once and retries once; a second
  401 is the error state.
- Closing the tab ends the session; with the password kind the session
  is also ended on the server (`POST …/protocol/openid-connect/logout`
  with the refresh token), so admin sessions do not pile up.
- Token endpoint errors in words: `invalid_client`, or Keycloak 26's
  `unauthorized_client` with *Invalid client or Invalid client
  credentials* ("the client id or secret is wrong"), `unauthorized_client` ("this client may not use
  client credentials: turn on Service Accounts"), `invalid_grant` with
  *Account is not fully set up* ("the admin has a required action
  pending (an OTP or a password change) - sign in to the admin console
  once, or use a service-account client"), any other `invalid_grant`
  ("wrong username or password"), and the server's
  `error_description` otherwise.
- **The admin API's audience (Keycloak 26.8, verified on the bench's
  26.8.2)**: the admin API checks the access token's audience. A service
  account with every realm-management role it needs still gets **401 on
  every admin call** until its client also has the realm's
  **`kc-admin-api` client scope** (the server logs *Expected audience not
  available in the token*). Such a token carries neither the
  `kc-admin-api` scope nor an `…/admin/realms/…` audience; after the
  one refresh and retry the provider reports `IdmError::Audience`:
  "open Clients → *client* → Client Scopes and add the realm's
  `kc-admin-api` client scope as Default". **Connecting a service
  account therefore needs, in the admin console:** Clients → the client
  → Settings → *Client authentication* on and *Service accounts roles*
  on; Service Account Roles → the `realm-management` roles below;
  Client Scopes → Add client scope → `kc-admin-api` (Default). New
  clients do not get `kc-admin-api` by default (the live test creates
  one and sees the 401). The password kind's `admin-cli` already has it.
- **What the account may do**: a service account needs
  `realm-management` client roles in the target realm (`view-users`,
  `manage-users`, `query-groups`, `view-clients`, `manage-clients`,
  `view-events`, `view-realm`, `manage-realm`…), or in master the
  `{realm}-realm` client's roles or the `admin` role. When the access
  token carries `resource_access`, those roles are read (the token is
  decoded, not verified - it is a hint) and controls the account cannot
  use are drawn disabled with the missing role in the tooltip. A token
  without role claims (a lightweight access token) gives no hints; the
  server's 403 is always the authority and is reported with the role
  that endpoint needs (§8.4).

## 6. Guards

### 6.1 Protected environments and read-only

- A profile whose environment is **protected** (Production and
  Production Support by default, docs/06 §3), or a profile marked
  read-only, opens its tab **read-only**: every write control is drawn
  disabled with the reason ("Production is read-only. Change it in
  Settings → Environments."), the `env_banner` and frame are shown, and
  the `Guarded` wrapper refuses every write method before a request is
  made. A new write verb is added to the wrapper's list in the same
  change that adds the verb; a test lists every write method and
  asserts each is refused (§10).
- Reads stay allowed on a protected tab, including viewing a client
  secret (it is still masked by default and copying is explicit).
- **Allow Writes for this tab** (owner, 2026-11-08, as the Database tool
  does): a protected tab's toolbar offers **Allow Writes**; it opens a
  confirmation naming the environment ("Allow writes to *Production*
  for this tab?") and, on yes, unlocks writes for this tab's session
  only (`guarded::WriteGate::unlock`). Reconnecting or closing the tab
  locks it again. A profile marked read-only cannot be unlocked.

### 6.2 Confirmations

| Verb | Guard |
|---|---|
| Delete User / Group / Client / Role | typed confirmation: the object's username, name or client id |
| Regenerate Client Secret | typed confirmation: the client id |
| Log Out All Sessions (realm) | typed confirmation: the realm's name |
| Log Out (a user's sessions), End Session | confirmation naming the user or session |
| Enable / Disable a user or client | confirmation naming it |
| Reset Password | the modal itself (§4.3), with the Temporary flag visible |
| Save of a multi-field form (Details, Attributes, Settings, a group or role) | a **review** of every changed field, old → new, before the request |
| Assign / Unassign Role, Join / Leave Group, Add / Remove Member | no dialog; the toast says what changed, and it is in the audit log |

The typed-confirmation modal is the shape `tool-queue`'s Delete already
draws (the name to type, OK disabled until it matches, the danger
colour). This is its second user, so it moves to `hs_ui::dialog` as a
shared helper rather than being copied (rust.md, "Reuse before
writing").

### 6.3 What is never offered

Deleting a realm, the built-in clients and roles of §4.5-§4.6,
impersonation, and any write on a read-only tab.

### 6.4 HyperStack's audit log

- Every write HyperStack sends - successful or refused by the server -
  appends one JSON line to `idm/audit.jsonl` in the workspace folder
  (`HyperStackData/workspaces/<name>/idm/audit.jsonl` by default; the
  workspace template's `.gitignore` lists it, like `layout.json`):

  ```json
  {"at":"2026-11-08T10:12:03Z","connection":"01JC…","server":"http://127.0.0.1:8180",
   "realm":"hs-demo","object":"user","id":"5b1c…","name":"alice",
   "verb":"reset-password","fields":["temporary"],"by":"hs-admin","os_user":"owner",
   "outcome":"ok"}
  ```

- `fields` holds **names** of what changed, never values; `by` is the
  client id or admin username the token was issued to; `outcome` is `ok`
  or the error in words. No password, secret, token or credential value
  is ever written, and a test asserts it (§10).
- The dashboard's Recent Changes reads it; **Clear History** truncates it
  behind a confirmation.
- It records what *this app* did. The server's own admin events (§4.8)
  remain the authoritative audit.

### 6.5 Secrets

- Client secrets: masked by default, **Show** for 30 s, **Copy** as an
  explicit click with the clipboard cleared after 60 s; never in a log
  line, history, a toast, the audit log or a screenshot-friendly default.
- Passwords typed for Reset Password: from the field to the request,
  zeroised after (§4.3).
- Tokens: memory only (§5.2). The `Debug` of every type that holds one
  prints no value (as `service_auth::Auth` does).
- JSON shown from the server (exports, admin event representations,
  More Settings) passes through one masking function that replaces the
  value of any key named `secret`, `clientSecret`, `password`, `value`
  under a credential, `bindCredential`, `privateKey`, `certificate`'s
  private part, and the identity providers' `clientSecret`.
- **The AI assistant never receives a secret value**: it is given the
  masked form of whatever it is told about.

## 7. Work and progress

- One **session task** per tab on the tokio runtime owns the provider
  session and its token; the tab sends commands through a bounded
  channel and drains answers at frame start (the shape of
  `tool-queue::runner`). Nothing blocks the UI thread; nothing calls
  `block_on`.
- **Server-side paging everywhere**: page size 50 by default (Settings →
  IDM, 20-200), `first` / `max` on every list; counts only from the
  count endpoints. No screen ever asks for a whole collection of users,
  group members or sessions.
- Reads show the list's loading state and are superseded rather than
  queued: a new search drops the answer of the old one (the request's
  generation). A read that has not answered in 15 s (Settings → Identity) is
  reported, not waited on.
- **Jobs** (`hs_core::jobs`, progress and cancel; cancel drops the
  request future, so the work actually stops): Export Realm; Include
  Sub-Group Users (walks a subtree); Show Effective roles when it needs
  one request per client; any write that takes more than one request
  (a role assignment across several clients).
- Lists are laid out once per answer and drawn with `show_rows`; nothing
  is re-parsed per frame.

## 8. Implementation notes

### 8.1 Crate and files

- `crates/tools/tool-idm/`, depending on `hs-core`, `hs-ui` and
  `hs-editor` (the read-only JSON view of §4.9 and §4.8) - never another
  tool crate.
  - `src/lib.rs` - `IdmFactory`, the tool, the dashboard;
  - `src/provider.rs` - `IdmProvider`, `Caps`, `Page`, `Paged`, the shared
    types; `src/guarded.rs` - the read-only wrapper and its write list;
  - `src/provider/keycloak/` - `auth.rs` (grants, refresh, token claims),
    `api.rs` (the calls of §4), `model.rs` (serde types, `#[serde(default)]`
    and unknown fields ignored so newer servers parse), `version.rs`
    (§8.3);
  - `src/runner.rs` - the session task; `src/audit.rs` - the audit log;
    `src/mask.rs` - §6.5's masking; `src/profile.rs` - the `[idm]` table;
  - `src/ui/` - one module per section (`users.rs`, `groups.rs`,
    `roles.rs`, `clients.rs`, `sessions.rs`, `events.rs`, `realm.rs`,
    `export.rs`) and `settings.rs` for Settings → Identity.
- HTTP through `hs_core::service_http::Http` (ADR-029): one
  `reqwest::Client` per tab; the bearer token is set per request by the
  session (the `Http` value is rebuilt or given the token on each call -
  `Http` gains a way to take a per-request `Auth`, and a **Verify TLS**
  switch that `Http::new` does not have yet, both in `hs-core`).
- The token request is form-encoded. `tool-idm` builds the body itself
  (each value percent-encoded, the buffer zeroised after the send) and
  enables `reqwest`'s `form` feature in its own `Cargo.toml`, as
  `tool-automation` does (the live test uses it); `serde_urlencoded` was
  already in `Cargo.lock`, so no new crate enters the lockfile.
  `cargo deny check` confirms it.
- JSON through `serde` / `serde_json`; the trait through `async-trait`,
  as `tool-tracker::driver` does.
- Registered in `crates/hyperstack/src/tools/mod.rs::factories()`
  **after Tracker** (before Database), category **Infra**, beside the
  other admin activities (Containers, Automation, Tracker). The rail is
  per workspace and user-ordered; this is the default (ADR-026).
- Descriptor: id `idm`, title key `tool-idm`, a Phosphor identification
  glyph (`IDENTIFICATION_BADGE` or `SHIELD_CHECK`, checked on screen),
  `accepts`: `OpenRequest::Connection` for `keycloak` profiles.
- Cross-tool: **Open in Admin Console** →
  `OpenRequest::Url { url: "{url}/admin/{auth_realm}/console/#/{realm}/…" }`
  (users/{id}/settings, clients/{id}/settings, groups, roles,
  realm-settings); **Open in Editor** → `OpenRequest::Text`.
- Every visible string in `locales/en.ftl` (Edit tool only), Title Case
  labels, sentence-case hints and toasts.

### 8.2 Data the app keeps

| What | Where | Secret? |
|---|---|---|
| The profile (§5.1) | `connections/keycloak/<name>.toml` in the workspace | no; the secret is a `secret:<ulid>` |
| Client secret, password, offline token | the vault | yes |
| Access and refresh tokens | memory, per tab | yes - never written |
| Audit log | `idm/audit.jsonl` in the workspace | no values (§6.4) |
| Tab state (realm, section, selection, column widths) | the layout (`ToolFactory::create` restores it) | no |

### 8.3 Version gating

`GET /admin/serverinfo` gives `systemInfo.version` (a Red Hat build's
`26.0.6.redhat-00001` reads as 26.0.6) and the features list (each
feature's name and whether it is enabled; `profileInfo` on servers
that lack the list). `Caps` is built from both:

| Server | Treated as |
|---|---|
| < 22 | connects with a *Version not supported* chip; only the reads are offered, best effort |
| **22-26** | supported, as the table below |
| > 26 | connects with a *Newer than tested* chip; everything offered, failures reported as usual |

| What | 22 | 23 | 24 | 25 | 26 |
|---|---|---|---|---|---|
| Sub-groups | from the parent's `subGroups` | `/groups/{id}/children`, `subGroupCount` | same | same | same |
| User attributes | free map (unless the user-profile preview is on) | same | **declarative user profile always on**: profile-driven fields, unmanaged-attribute policy (§4.3.1) | same | same |
| `userProfileMetadata` on GET user | - | - | yes | yes | yes |
| Organizations (phase 3) | hidden | hidden | hidden | shown only if the `ORGANIZATION` preview feature is enabled | shown when the feature is on and the realm has organizations enabled |
| Fine-grained admin permissions (phase 3) | v1, if enabled | v1 | v1 | v1 | v2 on 26.2+ when `ADMIN_FINE_GRAINED_AUTHZ_V2` is on and the realm enables admin permissions; v1 otherwise |

- **A realm-scoped account is not shown the version**: on the bench's
  26.8.2, the `hs-demo` service account's `serverinfo` has no
  `systemInfo` at all (only a master admin sees it). The version is then
  unknown (`Support::Unknown`, said in the header) and `Caps` comes from
  the features: a server that sends the `features` list is a recent one
  and is treated as 24+ (declarative user profile on); one that does not
  is treated as 22. When `serverinfo` is refused outright the same rule
  applies. Either way a 404 on a newer endpoint hides that feature for
  the session instead of failing the screen - `/groups/{id}/children`
  falls back to the parent's `subGroups` and `Caps::group_children`
  turns off.
- A feature that is off on the server is drawn as disabled with the
  reason, per the menu-items rule ("not hidden").

### 8.4 Errors

Through `service_http::status_error` / `transport_error`, then made
specific:

| Answer | Shown as |
|---|---|
| connect / DNS / TLS failure | "Nothing answered at host:port…" / the TLS reason, with *Verify TLS* named when the certificate is self-signed |
| 401 after one refresh | "The server refused the credentials." - the tab's error state with **Edit This Connection** |
| 403 | "Not allowed (403): this needs the realm-management role *manage-users*." - the role from a table per endpoint |
| 404 on an item | "This user no longer exists." - removed from the list |
| 404 on an endpoint | the feature hidden for the session (§8.3) |
| 409 | the server's `errorMessage` ("User exists with same username") |
| 400 on a save | the server's `errorMessage` / `field` shown beside the field it names when it names one |
| timeout | "host did not answer in 15 s" |

Every error lands in the screen's error state and, for a user's action,
a toast; none is swallowed into a log line. Log lines carry the method,
the path (ids, not names) and the status - never a body that could hold
a secret.

## 9. Keyboard

Every action has a palette command and a rebindable shortcut entry in
`hs_core::shortcuts`; most start unbound. Nothing on Ctrl+C/V/X,
Ctrl+Shift+V or Shift+Insert.

| Command | Id | Default |
|---|---|---|
| Identity: Refresh | `idm.refresh` | F5 (`run.alt`) |
| Identity: Search… (focus the section's search) | `idm.search` | Ctrl+Alt+U |
| Identity: Switch Realm… | `idm.realm` | Ctrl+Alt+Shift+R |
| Identity: Toggle Browse Column | `idm.browse` | — |
| Identity: Go to Users / Groups / Realm Roles / Client Roles / Clients / Sessions / Login Events / Admin Events / Realm Settings | `idm.go.users` … `idm.go.realm` | — |
| Identity: Enable User / Disable User | `idm.user.enable`, `idm.user.disable` | — |
| Identity: Reset Password… | `idm.user.reset_password` | — |
| Identity: Log Out User | `idm.user.logout` | — |
| Identity: Join Group… | `idm.user.join_group` | — |
| Identity: Assign Role… | `idm.assign_role` | — |
| Identity: Delete… (the selected object) | `idm.delete` | — |
| Identity: Show Client Secret / Copy Client Secret | `idm.client.show_secret`, `idm.client.copy_secret` | — |
| Identity: Regenerate Client Secret… | `idm.client.regenerate_secret` | — |
| Identity: Log Out All Sessions… | `idm.logout_all` | — |
| Identity: Export Realm… | `idm.export` | — |
| Identity: Open in Admin Console | `idm.console` | — |
| Identity: Add Identity Server | `idm.add` | — |
| Toggle AI Assistant | `idm.ai` | — |

Lists and the tree move with the arrow keys, Enter opens the item,
Delete asks **Delete…**, and Ctrl+F focuses the search where the list has
focus.

The assistant (ADR-019, review-only) is told the selected object with
secrets masked, and offers **Explain These Roles** (where a user's
effective roles come from), **Why Did This Login Fail?** (a login error
event) and **Summarise These Events**. It never sends a write.

## 10. Tests

- **Unit** (in the crate):
  - parsing: users, groups (22's `subGroups` and 23+'s children), roles,
    clients, sessions, login and admin events, `serverinfo` of 22, 24 and
    26 and a Red Hat build, from answers recorded against real servers;
  - paging: `first` / `max` built right, the status line from a count and
    from "more", a stale generation dropped;
  - version gating: `Caps` for each row of §8.3, and the 404 fallback;
  - auth: the three grants' bodies, refresh timing (renew at a third of
    `expires_in`, the 10 s floor), one refresh under concurrent requests,
    the 401-retry-once rule, the token endpoint errors in words, token
    claims read into permission hints and a lightweight token giving
    none;
  - guard decisions: every write method of `IdmProvider` refused by
    `Guarded` on a read-only tab, with a test that fails when a new
    write method is added without being listed;
  - read-merge-write: a save keeps fields it did not edit, including
    attributes;
  - masking: every key of §6.5 masked in nested JSON;
  - audit-log redaction: a reset password, a secret regeneration and a
    failed write append lines that carry no secret, password or token
    (the values are searched for in the file).
- **Live** (`tests/keycloak_live.rs`), skipped unless
  `HS_KEYCLOAK=http://127.0.0.1:8180` names a server: realm `hs-demo`,
  service-account client `hs-admin` (its secret from `HS_KEYCLOAK_SECRET`,
  the bench `.env`'s `KEYCLOAK_HS_ADMIN_SECRET`), and the password and
  offline-token kinds when `HS_KEYCLOAK_ADMIN` and
  `HS_KEYCLOAK_ADMIN_PASSWORD` are set (the bench's `KEYCLOAK_ADMIN_USER`
  and `KEYCLOAK_ADMIN_PASSWORD`). It connects with each kind,
  reads server info, pages users with a count, edits an attribute and
  reads it back, disables and enables, resets a temporary password,
  joins and leaves a group, assigns and unassigns a realm and a client
  role, reads and regenerates the secret of a throwaway client, lists
  and ends sessions, reads both event tables, exports the realm, and
  deletes the throwaway user, group, role and client it created through
  raw admin calls at the start. Afterwards it greps the data folder for
  the secret, the reset password and the tokens.
- **UI** (`egui_kittest`, where reachable without a server): the dashboard's
  empty state, Settings → Identity showing only the chosen kind's fields, a
  read-only tab's write controls disabled with the reason, the typed
  confirmation's OK disabled until the name matches.
- **The running app** on a scratch `--data-dir` against the bench
  Keycloak: every section of §4 walked at the default size and at a
  narrow window through *Checking a screen*, the double-click paths by
  hand, and a protected environment's tab refusing every write.

## 11. Decisions (owner, 2026-11-08)

The questions this section held are answered:

1. **Create in phase 1: yes** - users, groups (and sub-groups), realm
   and client roles, and clients (§4.10), beside view, edit and delete.
2. **Offline Token ships in phase 1** as the third kind (§5.2).
3. **Writes on a protected environment**: read-only by default, with a
   session **Allow Writes** toggle behind a confirmation naming the
   environment, as the Database tool does (§6.1).
4. **Audit log**: in the **workspace** folder,
   `<workspace>/idm/audit.jsonl`, field names only, never values; the
   workspace template's `.gitignore` lists it (§6.4).
5. **Keycloak's logo**: Devicon has none, so Simple Icons' mark,
   recoloured (§3.6).
6. **Custom CA**: an optional **CA File** field (PEM) beside Verify TLS
   (§5.1), through `hs_core::service_http::Tls`.
7. **Name**: **Identity** on the rail and in menus (`tool-idm =
   Identity`); IDM stays the crate and spec name.
8. **LDAP and Active Directory** are added to the providers, next after
   Keycloak (§4.12): "add LDAP and AD in the list".
9. **Apereo CAS is on hold** ("hold on CAS"). When it is picked up: CAS
   6.x / 7.x through its actuator endpoints (registered services, SSO
   sessions, audit log, health), 5.x through `/status`, and 3.x / 4.x
   only as a service-registry file editor - CAS stores no users, so
   Users, Groups and Roles would be disabled for it.

## What is implemented (2026-11-08)

The foundation (FEAT-0110 and the API halves of FEAT-0109 and
FEAT-0116); no screen of §4 yet.

- **`hs-core`**: `service_http::Tls` (Verify TLS and a CA file) with
  `Http::with_tls`, and `Http::request_as` for a per-request credential
  (the renewed bearer token); connection kind `keycloak` with
  `connections::IDM_KINDS`, `is_idm_kind` and `of_idm_kinds`; the
  workspace template's `.gitignore` lists `idm/audit.jsonl`.
- **`hs-ui`**: the Keycloak mark (`bytes://product/keycloak.svg`).
- **`tool-idm`**:
  - `provider.rs` - the `IdmProvider` trait (every phase-1 read and
    write of §4, creates included), the shared types, `Caps` with role
    hints and `section` / `write` availability, `Page` / `Paged` (one
    row over the page says whether there is more), `Change`
    (read-merge-write and the review), `ObjRef`, `Secret`, `IdmError`
    in words;
  - `provider/keycloak/` - `auth.rs` (the three grants, tokens in memory
    only, renewal at a third of the life with a 10 s floor, single-flight
    renewal, one retry on 401, the token endpoint's errors in words, the
    claims read for hints and the audience), `version.rs` (version parse
    with Red Hat builds, the 22-26 range, `Caps` per row of §8.3, the
    unknown-version rule), `model.rs` (serde types, unknown fields
    ignored, `raw` kept), `mod.rs` (every call, errors mapped: 401
    audience or credentials, 403 with the role, 404 gone or unsupported,
    409, 400 with its field, network);
  - `guarded.rs` - `Guarded` (refuses every write of `WRITES` on a
    read-only tab before any request, records every sent write) and
    `WriteGate` (profile read-only, protected environment, the session
    unlock);
  - `audit.rs` - `<workspace>/idm/audit.jsonl`, append, Recent Changes,
    Clear History, Remove;
  - `mask.rs` - one masking function for server JSON (exports, admin
    event representations, More Settings);
  - `profile.rs` - the `[idm]` table and its resolution into a grant;
  - `runner.rs` - `Session`: one task per tab, a bounded queue, work by
    topic with the newest superseding (and aborting) the older;
  - `ui.rs` - the descriptor (id `idm`, `IDENTIFICATION_BADGE`, Infra),
    the factory registered after Tracker, a placeholder dashboard and
    sidebar, and a tab that connects and says what it reached.
- **Tests**: unit tests for each module (45); `tests/mock.rs` against a
  scripted server (9: the grant body, 401-retry-once, the audience 401,
  token refusals, 403 / 404 / 409 / 400, read-merge-write, created ids,
  the Keycloak 22 sub-group fallback, a masked and a cancelled export);
  `tests/keycloak_live.rs` against the bench (reads of every section on
  `hs-demo`, 403 on `hs-other`, a create → update → delete round trip of
  a user, group and sub-group, client, client role and, as the master
  admin, a realm role; disable / enable, a temporary password reset,
  join / leave, assign / unassign, secret read and regenerate, the
  audience 401 on a client without `kc-admin-api`, the password and
  offline-token kinds; then the data folder searched for the secrets).
- **Not yet**: Settings → Identity, the tab's three columns and every
  §4 screen, the typed confirmation moved to `hs_ui::dialog`, the
  palette commands and shortcuts of §9, Recent Changes on the
  dashboard, the env banner and the Allow Writes control.
