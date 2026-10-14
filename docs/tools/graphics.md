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
| Transform | Resize to a width and height (keep aspect), scale by a factor, extend the canvas by padding on each side with a fill colour, crop, rotate 90°/180°/270°, flip horizontally/vertically |
| Adjust | Brightness, contrast, hue rotation, saturation, grayscale, invert, blur, sharpen (unsharp mask) |
| Icons | Pick a frame of a multi-size ICO to edit; build a multi-size ICO (16–256 px, PNG-compressed entries) from the current picture |
| Export | Save as PNG, JPEG (quality), WebP, BMP, GIF or ICO |

The canvas shows the picture on a checkerboard (transparency), with Fit,
100% and zoom buttons in the toolbar, and the info line
`original → current · size · file name`.

## 4. AI assistant

The review-only `hs_ui::ai_assistant::AssistantPanel` (toolbar **AI**)
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
