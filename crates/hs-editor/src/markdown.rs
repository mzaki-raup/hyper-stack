//! Markdown preview rendered with egui widgets (headings, paragraphs with
//! inline styles, lists, code blocks with highlighting, quotes, rules, links,
//! simple tables) and ```` ```mermaid ```` blocks drawn natively by
//! [`crate::mermaid`] (flowcharts and sequence diagrams; other kinds fall
//! back to the source with a note).

use egui::{Color32, RichText};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::buffer::Buffer;
use crate::highlight::LineHighlighter;
use crate::languages;

pub struct MarkdownStyle {
    pub dark: bool,
    pub text: Color32,
    pub muted: Color32,
    pub link: Color32,
    pub code_bg: Color32,
    pub quote_bar: Color32,
    pub base_size: f32,
}

#[derive(Default, Clone)]
struct Inline {
    bold: u32,
    italic: u32,
    code: u32,
    strike: u32,
    link: Option<String>,
}

/// A link inside a laid-out run of text: which characters it covers
/// (the job's text, in chars) and where it goes.
#[derive(Debug, Clone, PartialEq)]
struct Link {
    start: usize,
    end: usize,
    url: String,
}

/// A laid-out run of text with its links kept beside it, so a click on
/// the finished label can be answered with a destination. The job alone
/// only knows colours; it has no idea which coloured characters were a
/// link to what (2026-10-29).
#[derive(Default, Clone)]
struct Rich {
    job: egui::text::LayoutJob,
    links: Vec<Link>,
    chars: usize,
}

impl Rich {
    fn append(&mut self, text: &str, format: egui::TextFormat, link: Option<&str>) {
        let n = text.chars().count();
        if let Some(url) = link
            && n > 0
        {
            // Consecutive pieces of one link merge into one range.
            match self.links.last_mut() {
                Some(l) if l.end == self.chars && l.url == url => l.end += n,
                _ => self.links.push(Link {
                    start: self.chars,
                    end: self.chars + n,
                    url: url.to_string(),
                }),
            }
        }
        self.job.append(text, 0.0, format);
        self.chars += n;
    }

    fn is_empty(&self) -> bool {
        self.job.text.is_empty()
    }

    /// The link under character `index`, if any.
    fn link_at(&self, index: usize) -> Option<&str> {
        self.links
            .iter()
            .find(|l| l.start <= index && index < l.end)
            .map(|l| l.url.as_str())
    }
}

/// What a frame of [`render`] reports back.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MarkdownResponse {
    /// A link the reader clicked, as written in the document.
    pub link_clicked: Option<String>,
}

enum Block {
    Paragraph(Rich),
    Heading(HeadingLevel, Rich),
    Code {
        lang: String,
        text: String,
    },
    Rule,
    ListItem {
        depth: usize,
        marker: String,
        job: Rich,
    },
    Quote(Rich),
    TableRow {
        cells: Vec<Rich>,
        header: bool,
    },
}

/// Render `text` into `ui`.
/// Draw one run of text; a click on a link in it is reported through
/// `clicked`, and the cursor says a link is under it.
fn label(ui: &mut egui::Ui, rich: &Rich, clicked: &mut Option<String>) {
    if rich.links.is_empty() {
        ui.add(egui::Label::new(rich.job.clone()).wrap());
        return;
    }
    // Lay the job out at the width the label will get, so the galley
    // the click is tested against is the one on screen.
    let mut job = rich.job.clone();
    job.wrap.max_width = ui.available_width();
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let resp = ui.add(egui::Label::new(galley.clone()).sense(egui::Sense::click()));
    let at = |pos: egui::Pos2| {
        let index = galley.cursor_from_pos(pos - resp.rect.min).index.0;
        rich.link_at(index).map(str::to_string)
    };
    if let Some(pos) = resp.hover_pos()
        && at(pos).is_some()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
        && let Some(url) = at(pos)
    {
        *clicked = Some(url);
    }
}

pub fn render(ui: &mut egui::Ui, text: &str, style: &MarkdownStyle) -> MarkdownResponse {
    let blocks = parse(text, style, ui.available_width());
    let mut last_was_table = false;
    let mut clicked: Option<String> = None;
    for b in blocks {
        match b {
            Block::Paragraph(job) => {
                label(ui, &job, &mut clicked);
                ui.add_space(6.0);
            }
            Block::Heading(level, job) => {
                ui.add_space(if matches!(level, HeadingLevel::H1) {
                    10.0
                } else {
                    6.0
                });
                label(ui, &job, &mut clicked);
                if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
                    ui.separator();
                }
                ui.add_space(4.0);
            }
            Block::Code { lang, text } if lang.trim() == "mermaid" => {
                let pal = crate::mermaid::Palette {
                    text: style.text,
                    line: style.muted,
                    fill: style.code_bg,
                    frame: style.quote_bar,
                    note: Color32::from_rgba_unmultiplied(0xD9, 0xB0, 0x2A, 48),
                    label_bg: style.code_bg,
                };
                let shown = egui::Frame::new()
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        crate::mermaid::show(ui, &text, style.base_size, &pal)
                    })
                    .inner;
                if let Err(e) = shown {
                    ui.label(
                        RichText::new(format!("Mermaid: {e}. Showing the source."))
                            .color(style.muted)
                            .italics(),
                    );
                    code_block(ui, "", &text, style);
                }
                ui.add_space(6.0);
            }
            Block::Code { lang, text } => {
                code_block(ui, &lang, &text, style);
                ui.add_space(6.0);
            }
            Block::Rule => {
                ui.separator();
                ui.add_space(4.0);
            }
            Block::ListItem { depth, marker, job } => {
                ui.horizontal_top(|ui| {
                    ui.add_space(12.0 + depth as f32 * 18.0);
                    ui.label(RichText::new(marker).color(style.muted));
                    label(ui, &job, &mut clicked);
                });
            }
            Block::Quote(job) => {
                ui.horizontal_top(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(4.0, 20.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, style.quote_bar);
                    ui.add_space(6.0);
                    label(ui, &job, &mut clicked);
                });
                ui.add_space(4.0);
            }
            Block::TableRow { cells, header } => {
                ui.horizontal(|ui| {
                    for c in cells {
                        let mut job = c;
                        if header {
                            for s in &mut job.job.sections {
                                s.format.color = style.text;
                            }
                        }
                        ui.add_sized(
                            [ui.available_width() / 4.0_f32.max(1.0), 18.0],
                            egui::Label::new(job.job).truncate(),
                        );
                    }
                });
                last_was_table = true;
                continue;
            }
        }
        if last_was_table {
            ui.add_space(6.0);
            last_was_table = false;
        }
    }
    MarkdownResponse {
        link_clicked: clicked,
    }
}

/// A fenced code block with syntax highlighting.
fn code_block(ui: &mut egui::Ui, lang: &str, text: &str, style: &MarkdownStyle) {
    let frame = egui::Frame::new()
        .fill(style.code_bg)
        .inner_margin(egui::Margin::same(8))
        .corner_radius(egui::CornerRadius::same(4));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        let language = languages::by_extension(lang.trim());
        let mut hl = LineHighlighter::new(&language, style.dark);
        let buf = Buffer::from_text(text);
        let font = egui::FontId::monospace(style.base_size - 1.0);
        for line in 0..buf.len_lines() {
            let line_text = buf.line_text(line);
            if line + 1 == buf.len_lines() && line_text.is_empty() {
                break;
            }
            let mut job = egui::text::LayoutJob::default();
            let spans = hl.spans(&buf, line);
            if spans.is_empty() {
                job.append(
                    &line_text,
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: style.text,
                        ..Default::default()
                    },
                );
            } else {
                let chars: Vec<char> = line_text.chars().collect();
                for s in spans {
                    let piece: String = chars[s.start.min(chars.len())..s.end.min(chars.len())]
                        .iter()
                        .collect();
                    job.append(
                        &piece,
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: s.fg,
                            ..Default::default()
                        },
                    );
                }
            }
            ui.add(egui::Label::new(job).wrap());
        }
    });
}

fn fmt(style: &MarkdownStyle, inline: &Inline, size: f32, heading: bool) -> egui::TextFormat {
    let font_id = if inline.code > 0 {
        egui::FontId::monospace(size - 1.0)
    } else {
        egui::FontId::proportional(size)
    };
    let color = if inline.link.is_some() {
        style.link
    } else {
        style.text
    };
    let mut f = egui::TextFormat {
        font_id,
        color,
        ..Default::default()
    };
    if inline.italic > 0 {
        f.italics = true;
    }
    if inline.bold > 0 || heading {
        f.color = color;
        // egui has no bold weight toggle for proportional fonts; use a slightly larger size.
        f.font_id = egui::FontId::proportional(size * if heading { 1.0 } else { 1.04 });
        if inline.code > 0 {
            f.font_id = egui::FontId::monospace(size - 1.0);
        }
    }
    if inline.code > 0 {
        f.background = style.code_bg;
    }
    if inline.strike > 0 {
        f.strikethrough = egui::Stroke::new(1.0, color);
    }
    if inline.link.is_some() {
        f.underline = egui::Stroke::new(1.0, style.link);
    }
    f
}

/// Split prose into plain pieces and bare `http(s)://` addresses, each
/// address carrying itself as its destination. An address ends at
/// whitespace; a trailing `.`, `,`, `;`, `:`, `)` or quote is prose,
/// not part of it, which is what a sentence ending in a URL needs.
fn autolink(text: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut rest = text;
    loop {
        let hit = ["https://", "http://"]
            .iter()
            .filter_map(|p| rest.find(p))
            .min();
        let Some(i) = hit else {
            if !rest.is_empty() {
                out.push((rest.to_string(), None));
            }
            break;
        };
        if i > 0 {
            out.push((rest[..i].to_string(), None));
        }
        let tail = &rest[i..];
        let mut end = tail.find(char::is_whitespace).unwrap_or(tail.len());
        while end > 0
            && matches!(
                &tail[end - 1..end],
                "." | "," | ";" | ":" | ")" | "\"" | "'"
            )
        {
            end -= 1;
        }
        let url = &tail[..end];
        if url.len() <= "https://".len() {
            // "http://" alone is not an address.
            out.push((url.to_string(), None));
        } else {
            out.push((url.to_string(), Some(url.to_string())));
        }
        rest = &tail[end..];
    }
    out
}

fn parse(text: &str, style: &MarkdownStyle, _width: f32) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(text, opts);
    let mut blocks = Vec::new();
    let mut job = Rich::default();
    let mut inline = Inline::default();
    let mut heading: Option<HeadingLevel> = None;
    let mut list_depth = 0usize;
    let mut list_counters: Vec<Option<u64>> = Vec::new();
    let mut in_item = false;
    let mut in_quote = false;
    let mut code: Option<(String, String)> = None;
    let mut table_cells: Vec<Rich> = Vec::new();
    let mut in_table_header = false;
    let mut in_table = false;
    let base = style.base_size;

    let heading_size = |l: HeadingLevel| match l {
        HeadingLevel::H1 => base * 1.8,
        HeadingLevel::H2 => base * 1.5,
        HeadingLevel::H3 => base * 1.25,
        HeadingLevel::H4 => base * 1.1,
        _ => base,
    };
    let size_now = |heading: Option<HeadingLevel>| heading.map(heading_size).unwrap_or(base);

    for ev in parser {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { level, .. } => heading = Some(level),
                Tag::BlockQuote(_) => in_quote = true,
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    list_depth += 1;
                    list_counters.push(start);
                }
                Tag::Item => in_item = true,
                Tag::Emphasis => inline.italic += 1,
                Tag::Strong => inline.bold += 1,
                Tag::Strikethrough => inline.strike += 1,
                Tag::Link { dest_url, .. } => inline.link = Some(dest_url.to_string()),
                Tag::Table(_) => in_table = true,
                Tag::TableHead => in_table_header = true,
                Tag::TableRow => {}
                Tag::TableCell => {}
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => {
                    if in_item {
                        // paragraph inside a list item: flushed at item end
                    } else if in_quote {
                        blocks.push(Block::Quote(std::mem::take(&mut job)));
                    } else if !job.is_empty() {
                        blocks.push(Block::Paragraph(std::mem::take(&mut job)));
                    }
                }
                TagEnd::Heading(level) => {
                    blocks.push(Block::Heading(level, std::mem::take(&mut job)));
                    heading = None;
                }
                TagEnd::BlockQuote(_) => {
                    if !job.is_empty() {
                        blocks.push(Block::Quote(std::mem::take(&mut job)));
                    }
                    in_quote = false;
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, text)) = code.take() {
                        blocks.push(Block::Code { lang, text });
                    }
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    list_counters.pop();
                }
                TagEnd::Item => {
                    let marker = match list_counters.last_mut() {
                        Some(Some(n)) => {
                            let m = format!("{n}.");
                            *n += 1;
                            m
                        }
                        _ => "•".to_string(),
                    };
                    blocks.push(Block::ListItem {
                        depth: list_depth.saturating_sub(1),
                        marker,
                        job: std::mem::take(&mut job),
                    });
                    in_item = false;
                }
                TagEnd::Emphasis => inline.italic = inline.italic.saturating_sub(1),
                TagEnd::Strong => inline.bold = inline.bold.saturating_sub(1),
                TagEnd::Strikethrough => inline.strike = inline.strike.saturating_sub(1),
                TagEnd::Link => inline.link = None,
                TagEnd::TableHead => {
                    blocks.push(Block::TableRow {
                        cells: std::mem::take(&mut table_cells),
                        header: true,
                    });
                    in_table_header = false;
                }
                TagEnd::TableRow => {
                    blocks.push(Block::TableRow {
                        cells: std::mem::take(&mut table_cells),
                        header: false,
                    });
                }
                TagEnd::TableCell => {
                    table_cells.push(std::mem::take(&mut job));
                }
                TagEnd::Table => in_table = false,
                _ => {}
            },
            Event::Text(t) => {
                if let Some((_, body)) = code.as_mut() {
                    body.push_str(&t);
                } else if inline.link.is_some() {
                    let f = fmt(
                        style,
                        &inline,
                        size_now(heading),
                        heading.is_some() || (in_table && in_table_header),
                    );
                    job.append(&t, f, inline.link.as_deref());
                } else {
                    // A bare `https://…` in prose is a link too: a note
                    // full of addresses nobody wrapped in brackets should
                    // still open them (owner's requirement, 2026-10-29).
                    let strong = heading.is_some() || (in_table && in_table_header);
                    for (piece, url) in autolink(&t) {
                        let mut i = inline.clone();
                        i.link = url.clone();
                        let f = fmt(style, &i, size_now(heading), strong);
                        job.append(&piece, f, url.as_deref());
                    }
                }
            }
            Event::Code(t) => {
                inline.code += 1;
                let f = fmt(style, &inline, size_now(heading), false);
                job.append(&t, f, None);
                inline.code -= 1;
            }
            Event::SoftBreak => {
                let f = fmt(style, &inline, size_now(heading), false);
                job.append(" ", f, None);
            }
            Event::HardBreak => {
                let f = fmt(style, &inline, size_now(heading), false);
                job.append("\n", f, None);
            }
            Event::Rule => blocks.push(Block::Rule),
            Event::TaskListMarker(done) => {
                let f = fmt(style, &inline, base, false);
                job.append(if done { "☑ " } else { "☐ " }, f, None);
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                let f = egui::TextFormat {
                    font_id: egui::FontId::monospace(base - 1.0),
                    color: style.muted,
                    ..Default::default()
                };
                job.append(&h, f, None);
            }
            _ => {}
        }
    }
    if !job.is_empty() {
        blocks.push(Block::Paragraph(job));
    }
    blocks
}

/// Minimal HTML escaping for the export helpers.
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Markdown as HTML for Print…, with tables, strikethrough, task lists and
/// footnotes on. Mermaid fences become `<pre class="mermaid">` blocks, which
/// the page renders with mermaid.js when it is online and shows as text
/// otherwise.
pub fn to_html(text: &str) -> String {
    use pulldown_cmark::{Options, Parser, html};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    let parser = Parser::new_ext(text, opts);
    let mut out = String::with_capacity(text.len() * 2);
    html::push_html(&mut out, parser);
    let out = out.replace(
        "<pre><code class=\"language-mermaid\">",
        "<pre class=\"mermaid\"><code class=\"language-mermaid\">",
    );
    format!(
        "<div class=\"md\">{out}</div>\n<script type=\"module\">try {{ const m = await import('https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs'); for (const el of document.querySelectorAll('pre.mermaid')) {{ el.textContent = el.textContent; }} m.default.initialize({{ startOnLoad: true, theme: document.documentElement.dataset.scheme === 'dark' ? 'dark' : 'default' }}); }} catch (e) {{ }}</script>"
    )
}

#[cfg(test)]
mod tests {
    /// A bracketed link and a bare address both keep their destination
    /// beside the characters they cover, so a click can be answered.
    #[test]
    fn links_keep_their_ranges_and_bare_addresses_count() {
        let style = super::MarkdownStyle {
            dark: true,
            text: egui::Color32::WHITE,
            muted: egui::Color32::GRAY,
            link: egui::Color32::LIGHT_BLUE,
            code_bg: egui::Color32::BLACK,
            quote_bar: egui::Color32::GRAY,
            base_size: 14.0,
        };
        let blocks = super::parse(
            "See [docs](https://example.com/d) or https://example.org/x, then go.",
            &style,
            400.0,
        );
        let super::Block::Paragraph(rich) = &blocks[0] else {
            panic!("a paragraph")
        };
        assert_eq!(rich.links.len(), 2);
        assert_eq!(rich.links[0].url, "https://example.com/d");
        assert_eq!(
            rich.links[1].url, "https://example.org/x",
            "the comma is prose"
        );
        // "See " is four chars; "docs" covers 4..8.
        assert_eq!((rich.links[0].start, rich.links[0].end), (4, 8));
        assert_eq!(rich.link_at(5), Some("https://example.com/d"));
        assert_eq!(rich.link_at(2), None);
        assert_eq!(
            super::autolink("x http://a.b/c) y"),
            vec![
                ("x ".to_string(), None),
                ("http://a.b/c".to_string(), Some("http://a.b/c".to_string())),
                (") y".to_string(), None),
            ]
        );
    }

    use super::*;

    fn style() -> MarkdownStyle {
        MarkdownStyle {
            dark: true,
            text: Color32::WHITE,
            muted: Color32::GRAY,
            link: Color32::LIGHT_BLUE,
            code_bg: Color32::DARK_GRAY,
            quote_bar: Color32::GRAY,
            base_size: 14.0,
        }
    }

    #[test]
    fn blocks_are_recognised() {
        let md = "# Title\n\nSome **bold** and `code`.\n\n- a\n- b\n\n1. one\n2. two\n\n```rust\nfn x() {}\n```\n\n> quoted\n\n---\n";
        let blocks = parse(md, &style(), 600.0);
        let kinds: Vec<&str> = blocks
            .iter()
            .map(|b| match b {
                Block::Heading(..) => "h",
                Block::Paragraph(_) => "p",
                Block::ListItem { .. } => "li",
                Block::Code { .. } => "code",
                Block::Quote(_) => "q",
                Block::Rule => "hr",
                Block::TableRow { .. } => "tr",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["h", "p", "li", "li", "li", "li", "code", "q", "hr"]
        );
        if let Block::ListItem { marker, .. } = &blocks[4] {
            assert_eq!(marker, "1.");
        }
        if let Block::Code { lang, text } = &blocks[6] {
            assert_eq!(lang, "rust");
            assert_eq!(text, "fn x() {}\n");
        }
    }
}
