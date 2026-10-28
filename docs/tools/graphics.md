# Tool spec — Graphics

**Crate:** `tool-graphics` · **Priority:** P1 (owner's request, 2026-10-15) · **Status:** implemented

## 1. Purpose

The picture chores that come up next to code: look at an image from the
project, convert it to another format, scale it for a README, pad it onto a
canvas, make a favicon or a multi-size `.ico` from a logo, pull the frames
out of an icon, and the basic colour adjustments. Everything runs locally
with the `image` crate; nothing is uploaded.

## 2. Opening a picture

- Click a picture in the editor's project tree (`png`, `jpg`, `jpeg`,
  `webp`, `bmp`, `gif`, `ico`): the editor hands it to the graphics tool
  instead of showing bytes (`hs_core::sniff::is_image_path`).
- Sidebar → **Open Image…**, or the Graphics card on the Home tab.
- A tab shows one picture; opening a second picture reuses a tab with no
  pending operations or opens another.

## 3. Operations

Operations stack on the original and are listed in the sidebar, each with
a remove button; **Remove Last** and **Remove All** undo. The picture is
re-rendered from the original whenever the list changes
(`tool_graphics::ops::apply_all`).

| Group | Operations |
|---|---|
| Transform | Resize to a width and height (keep aspect), scale by a factor, extend the canvas by padding on each side with a fill colour, crop (with a **placeholder on the canvas**: Select on Canvas shows a rectangle with grips and rule-of-thirds guides; drag inside to move, an edge or corner to resize, outside to draw a new one; the X/Y/W/H boxes follow and Apply crops; the grip is judged from the press position and the rectangle is clamped to the current picture), rotate 90°/180°/270°, flip horizontally/vertically |
| Adjust | Brightness, contrast, hue rotation, saturation, blur, sharpen (unsharp mask) as **live sliders**: moving one re-renders the picture (a preview layer on top of the committed operations, debounced 60 ms); Apply commits that slider's value as an operation and resets it; Save As / Save ICO commit the preview first. Grayscale and Invert are toggles (on/off state shown, a second click removes the operation) |
| Icons | Pick a frame of a multi-size ICO to edit; build a multi-size ICO (16–256 px, PNG-compressed entries) from the current picture |
| Export | Save as PNG, JPEG (quality), WebP, BMP, GIF or ICO |

The canvas shows the picture on a checkerboard (transparency), with Fit,
100% and zoom buttons in the toolbar, and the info line
`original → current · size · file name`.

The sidebar is laid out on aligned rows (`Rows`, one `egui::Grid` per
section: a fixed label column, the controls, an Apply column) under small
group headings; long control rows wrap inside their column so the panel
never grows. The Adjust group uses `slider_block`: label, current value and
Apply on one line, the slider full width underneath, with a visible rail
on dark themes.

## 4. AI assistant

The `hs_ui::ai_assistant::AssistantPanel` (toolbar **AI Assistant**, quick
actions Explain This Image, Suggest Edits, Best Export Format)
receives the picture's metadata and the operation list, never the pixels,
so it can suggest what to do ("make it 512 px wide, then sharpen") for the
user to apply by hand. Insert is disabled in this tool.

## 5. Print and Snip

Print… exports the current rendering as an HTML page with the picture
embedded (PNG, Base64); Snip captures the tab as it is on screen.

## 6. Tests

`ops.rs` has known-answer and round-trip tests: geometry (resize with one
side 0, scale, extend, crop clamping, rotation), colour (grayscale,
saturation, invert, chained operations), every encoder decoding back to
the same size, multi-size ICO assembly and frame extraction.

## Addendum (2026-10-17): one tab per image

Opening an image gives it **its own tab**. A Graphics tab that already
holds a picture no longer claims the next file opened — `uri_affinity`
answers only while the tab is empty — so Open Image… on a loaded tab opens
a second one beside it rather than replacing the work in front of you.
Opening a file that is already open focuses that tab instead of making a
duplicate, which falls out of the same rule: the tab holding that path
claims it. Both paths go through `Services::open`, so a file arriving from
elsewhere in the app behaves the same way. Owner's requirement, 2026-10-17.

## Addendum (2026-10-19): four tools in one activity

The Graphics activity is being split into **Image**, **Icon**, **Diagram
Editor** and **Diagram Generator**, with a permanent dashboard home tab and an
activity sidebar that lists all four plus a scrollable Recent section. The
picture editor described above becomes the Image tab; the Icon tab keeps the
ICO operations and adds colour-key transparency. The diagram suite is specified
separately in [diagrams.md](diagrams.md); it is two releases (flowchart and
architecture first, ERD and sequence plus imports second). This addendum only
records the split: the existing image operations and their tests remain the
Image tab's behaviour.

**Implemented 2026-10-19:** the activity shell, the dashboard and Recent list,
the separate Image and Icon tabs with the settings in an in-pane panel, and
Convert to Icon. Colour-key transparency and the diagram panes are pending.

## Addendum (2026-10-22): the picture goes to the assistant, and pictures from prompts

**Pictures in questions.** `hs_core::ai::Message` carries `images:
Vec<ImagePart>` (MIME type and bytes). Every provider's request builder
sends them: an OpenAI-compatible service gets a content array with
`image_url` data URIs, Anthropic a base64 `image` block, Gemini an
`inline_data` part, Bedrock an `image` block, and 1min.AI an asset upload
(`POST /api/assets`, multipart built by hand since the workspace's reqwest
has no `multipart` feature) whose key goes into
`promptObject.attachments.images`. The picture tab sets
`AssistantPanel::attachment` to a PNG of what is shown, downscaled to
1024 px on a side and encoded once per render (`render_version`), so
"Explain this Image" describes the picture rather than its dimensions.

**Generate Picture.** The assistant column of a picture tab opens with a
Generate Picture form: provider (only kinds whose `generates_images()` is
true - 1min.AI), image model (Stability AI's Stable Diffusion XL 1.0,
`stable-diffusion-xl-1024-v1-0`, by default - owner's choice; any 1min.AI
image model name can be typed, and an SDXL model gets one of its nine fixed
sizes, the closest in aspect to the current picture), the prompt, and
"Use the current picture
as the source". `hs_core::ai::generate_image` posts `IMAGE_GENERATOR`
(prompt, aspect ratio from the current size) or, with a source,
`IMAGE_VARIATOR` (the uploaded asset's key and the prompt) to
`/api/features`, then downloads `aiRecord.temporaryUrl`. The bytes are
written as a draft under `HyperStackData/graphics/drafts/images/` and the
tab loads that file like any picture, so Undo, Save As and Convert to
Icon all apply. Chat and pictures may come from different providers: the
chat uses the assistant's provider, the form its own.

**Edit, Enhance and Upscale (2026-10-22, split 2026-10-23).** The form
has four jobs (`hs_core::ai::ImageJob`), each with its own model field:
**Generate** as above; **Edit** sends the current picture and the prompt as
`IMAGE_VARIATOR` - Dzine by default, the documented 1min.AI variator that
takes a prompt, with a Strength slider sent as `structure_match`
(inverted), `style_intensity` and `image_strength` so Dzine and any
Stability-style model both read it; **Enhance** and **Upscale** are
described below. 1min.AI documents
no image-to-image route for Stable Diffusion XL, so the owner's request to
edit with SDXL 1.0 is met as closely as the service allows: SDXL stays the
generation model and the edit model is a typed field. The result lands as
a draft like a generated picture, so the edited or enhanced picture is a
new tab beside the original.

**Upscale is not Enhance (owner's rule, 2026-10-23).** Until this date
every backend's "Enhance" ran an upscaler, which is the wrong operation
under the right name, and there was no way to ask for the other one.
They are now two jobs with two cards of settings:

- **Upscale** makes more pixels and leaves the picture alone: a
  super-resolution model, and how much bigger as either a **ratio** or a
  **target width and height**, because servers take one shape or the
  other (Fooocus and the Extras route a multiple of the source, ComfyUI's
  `ImageScale` a size). `ImageWorkflow::upscale_target` and
  `upscale_ratio` answer whichever a backend needs; a 0 axis in the size
  keeps the source's own value there.
- **Enhance** makes a better picture at the same size: a low-denoise
  refiner pass (**Refiner Strength**, **Refiner Steps**) and the server's
  face restorer (**Restore Faces**, **Restorer Strength**). It never
  resizes.

Per backend: on a **WebUI** an Upscale is `extra-single-image` with
`resize_mode` 0 and `upscaling_resize`, or 1 with `upscaling_resize_w/h`,
while an Enhance is img2img at the refiner's `denoising_strength` with
`restore_faces`, falling back to `extra-single-image` at a resize of 1
with `gfpgan_visibility`/`codeformer_visibility` when the refiner is 0.
On **ComfyUI** they are two workflows: `COMFY_UPSCALE`
(`UpscaleModelLoader` → `ImageUpscaleWithModel` → `ImageScale`) and
`COMFY_ENHANCE` (`VAEEncode` → `KSampler` at the refiner denoise →
`VAEDecode`), each with its own slot on the provider page, and
`{{scale_by}}` joins the placeholders for a workflow that prefers
`ImageScaleBy`. On **SwarmUI** both are init-image passes, the Upscale at
the larger size and the Enhance at the source's. On **InvokeAI** the
`esrgan` node is the Upscale and the `i2l` img2img graph the Enhance. On
**Fooocus** `Upscale (1.5x|2x)` against `Vary (Subtle)`. On **1min.AI**
`IMAGE_UPSCALER` against `IMAGE_VARIATOR` held close to the source, since
the service has no restoration feature of its own.

**Self-hosted image servers (2026-10-22).** The same four jobs run on a
server of the user's own, added in Settings → AI Assistant under Image
Providers (`hs_core::ai_local`, owner's request): the **Stable Diffusion
WebUI API** kind (AUTOMATIC1111, SD.Next, Forge, or a Stability Matrix
package of one of them; `/sdapi/v1/txt2img`, `img2img` with
`denoising_strength` = Strength, `extra-single-image` with the provider's
upscaler and factor; `--api-auth` as Basic), **ComfyUI** (also SwarmUI's
`/ComfyBackendDirect` and Stability Matrix's package; `/upload/image`,
`/prompt`, polling `/history`, `/view`; driven by a workflow in the API
JSON form with `{{prompt}}`-style placeholders - built-in ones for the
four jobs, `ai_local::COMFY_*`, or the user's own pasted per job),
**SwarmUI** (`/API/GetNewSession`, `GenerateText2Image` with `initimage`
+ `initimagecreativity`; Upscale is an init-image pass at the target
size, Enhance one at the source's size), **InvokeAI** (`/api/v2/models/` to resolve the model
record, a small graph - `main_model_loader`/`compel` or
`sdxl_model_loader`/`sdxl_compel_prompt` by the model's base, `noise`,
`denoise_latents`, `i2l` for edits and enhancements, `l2i`; `esrgan` for
Upscale - queued
through `/api/v1/queue/default/enqueue_batch` and polled) and **Fooocus**
behind Fooocus-API (`/v1/generation/text-to-image` with an SDXL preset
size, `/v2/generation/image-upscale-vary` as Vary Subtle/Strong or
Upscale 1.5x/2x). Every provider of these kinds carries an
`ImageWorkflow` (checkpoint, negative prompt, steps, CFG, sampler,
scheduler, seed, the Upscale card's model, mode, ratio and target size,
the Enhance card's refiner and restorer, ComfyUI workflows, extra JSON
merged into every body), edited on the provider's page; the Graphics form
shows a Checkpoint field (empty = the provider's) instead of a 1min.AI
model id and no model row for a self-hosted Upscale, which runs the
upscale model rather than a checkpoint. A provider saved before these
settings existed reads back with the working defaults, because
`ImageWorkflow` carries a container-level `serde(default)`
(`an_older_provider_gains_the_new_image_settings`). The request shapes follow the
projects' own API documentation as read on 2026-10-22. None of the servers
is installed on the build machine, so they are exercised against **mock
servers** (`crates/hs-core/tests/ai_local_mock.rs`: a std-thread HTTP/1.1
server per test answering the documented shapes) that check every field
this client sends - txt2img/img2img/upscale bodies and Basic auth for the
WebUI; the multipart upload, `/prompt` workflow with the server's first
checkpoint, `/history` polling and `/view` download for ComfyUI; the
session, `swarm_token` cookie and init-image creativity for SwarmUI; the
SD-1 and SDXL graphs, `i2l`, `esrgan` and the queue poll for InvokeAI; the
preset sizes and Vary/Upscale methods for Fooocus; that an Enhance never
resizes and an Upscale always does, on all five; plus the extra JSON
merge and server-error reporting. `f32` settings are rounded to four
decimals at the JSON boundary (`ai_local::tidy`), so a Strength of 0.7 is
sent as `0.7`. A live run against a real server remains the owner's
check. A self-hosted job may take minutes on a CPU: its client waits
twenty.

**Dashboard and sidebar (2026-10-22).** The Functions row's buttons carry
the same glyph as the What To Do tiles (`Function::action_icon`: Open
Image…, Open Icon…, New Diagram…, New Generation…), and a left click on
the sidebar's Image row opens a menu with Open Image… and New Image…, the
same two the right click offers, rather than guessing which was meant.

The four rows are drawn with `hs_ui::form::list_row`, the row the Devtools
and Settings sidebars use (owner's request, 2026-10-23): a 20 px icon, a
comfortable row height, space above and below each row, and the row whose
tool the open tab belongs to marked as selected. A disabled row keeps the
"coming soon" hover text.

**New Image.** From the Image row's context menu in the sidebar, the
dashboard's New Image tile, the Functions row's second button or the
palette: width, height and a background colour (or transparent) make a
blank PNG draft in the same folder, opened in a new tab.

## Dashboard (2026-10-29)

The activity's home tab wears the shape every activity now shares
(`hs_ui::dashboard::saved_dashboard`): a band with the activity's
mark, name and subtitle; the figures in equal rounded boxes;
**Start Here** with its first moves; **Functions** - the function list in its own card, with Start Here above it;
and the shared **Open Here** grid, each section ruled apart.
