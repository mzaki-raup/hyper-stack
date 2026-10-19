# Archify schema adaptation

These JSON Schema files are adapted from Archify v3.2.0 (MIT), commit
`208acba8d412335185b2056fedf54b3405df43fb`. The full MIT text and both
copyright lines are in [`THIRD_PARTY_NOTICES.md`](../../../THIRD_PARTY_NOTICES.md).

They are the contract for the **semantic diagram IR** used by the Diagram
Generator and, in Release 2, by Archify JSON import. They are not the editor's
`.hdiag` format: `.hdiag` stores absolute geometry and remains editor-native.

## What was kept

| File | Definitions |
|---|---|
| `common.schema.json` | `id`, `point`, `side`, `componentType`, `variant`, `relationshipWidth` |
| `workflow.schema.json` | `meta.title/subtitle`, `lanes`, `nodes`, `edges`, `groups` |
| `architecture.schema.json` | `meta.title/subtitle`, `layout`, `components`, `boundaries`, `connections` |
| `erd.schema.json` (Release 2) | entities, attributes, keys, relationships |
| `sequence.schema.json` (Release 2) | participants, messages, fragments |

## What was dropped

`meta.output`, `meta.locale`, `meta.translations`, `meta.animation`,
`meta.visual_preset`, `meta.quality_profile`, `meta.engineering_profile`,
`meta.repository`, `meta.views`, `meta.legend`, `meta.viewBox`,
`meta.cards`, `phases`, `mainPath`, `semanticChecks`, `nodeIcon`,
`brandMark`, `sourceReferences`, `portableOutputPath`, the HTML output-path
rules, `components.pos`/`components.size`, and every renderer/viewer asset.

The renderer is an independent native implementation. No Archify HTML, CSS,
SVG, JavaScript, fonts, brand marks or generated validators were copied.

## Import mode

Archify JSON files may carry the dropped fields. The importer accepts them,
ignores the HTML/output fields, honors `pos`/`size`/`via`/`viewBox` when they
are present, and reports what it ignored. The strict IR used by the generator
does not accept them.

## Changing the pin

Moving to a newer Archify release is a deliberate change: update the tag and
commit here, in `THIRD_PARTY_NOTICES.md`, and in ADR-025, then re-run the
compatibility fixtures.
