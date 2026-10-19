# Third-party notices

HyperStack includes material adapted from Archify.

## Archify

Source: <https://github.com/tt-a1i/archify>
Version: v3.2.0
Commit: `208acba8d412335185b2056fedf54b3405df43fb`
Adapted: 2026-10-19

The JSON Schema definitions and validation rules under
`crates/hs-diagram/schema/archify/` are adapted from Archify's schemas. The
renderer is an independent implementation; no Archify renderer, viewer, HTML,
CSS, SVG generation, brand-mark data, fonts or generated validators were
copied.

Adapted files:

- `archify/schemas/common.schema.json` — the `id`, `point`, `side`,
  `componentType`, `variant` and `relationshipWidth` definitions.
- `archify/schemas/workflow.schema.json` — lanes, nodes, edges and groups
  (Release 1).
- `archify/schemas/architecture.schema.json` — layout, components, boundaries
  and connections (Release 1).
- `archify/schemas/erd.schema.json` — entities, attributes, keys and
  relationships (Release 2).
- `archify/schemas/sequence.schema.json` — participants, messages and
  fragments (Release 2).

The upstream example JSON files are used as compatibility fixtures when the
importer lands (Release 2); they will be listed here then.

Changes: reduced to the subset HyperStack's diagram IR uses; removed the
HTML- and agent-only fields (`meta.output`, `meta.views`, `meta.sources`,
`meta.repository`, `meta.translations`, `meta.brand`, `meta.legend`,
`meta.cards`, animation, visual preset, quality profile and the HTML
output-path rules); validation rules were reimplemented in Rust.

The following licence applies to the adapted material above.

MIT License

Copyright (c) 2026 tt-a1i (Archify)
Copyright (c) 2025 Cocoon AI

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
