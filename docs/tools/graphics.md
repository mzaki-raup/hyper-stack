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
