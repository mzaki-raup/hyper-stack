//! Markdown preview rendered with egui widgets (headings, paragraphs with
//! inline styles, lists, code blocks with highlighting, quotes, rules, links,
//! simple tables). Mermaid blocks are shown as code for now.

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

#[derive(Default)]
struct Inline {
    bold: u32,
    italic: u32,
    code: u32,
    strike: u32,
    link: Option<String>,
}

enum Block {
    Paragraph(egui::text::LayoutJob),
    Heading(HeadingLevel, egui::text::LayoutJob),
    Code {
        lang: String,
        text: String,
    },
    Rule,
    ListItem {
        depth: usize,
        marker: String,
        job: egui::text::LayoutJob,
    },
    Quote(egui::text::LayoutJob),
    TableRow {
        cells: Vec<egui::text::LayoutJob>,
        header: bool,
    },
}

/// Render `text` into `ui`.
pub fn render(ui: &mut egui::Ui, text: &str, style: &MarkdownStyle) {
    let blocks = parse(text, style, ui.available_width());
    let mut last_was_table = false;
    for b in blocks {
        match b {
            Block::Paragraph(job) => {
                ui.add(egui::Label::new(job).wrap());
                ui.add_space(6.0);
            }
            Block::Heading(level, job) => {
                ui.add_space(if matches!(level, HeadingLevel::H1) {
                    10.0
                } else {
                    6.0
                });
                ui.add(egui::Label::new(job).wrap());
                if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
                    ui.separator();
                }
                ui.add_space(4.0);
            }
            Block::Code { lang, text } => {
                let frame = egui::Frame::new()
                    .fill(style.code_bg)
                    .inner_margin(egui::Margin::same(8))
                    .corner_radius(egui::CornerRadius::same(4));
                frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let language = languages::by_extension(lang.trim());
                    let mut hl = LineHighlighter::new(&language, style.dark);
                    let buf = Buffer::from_text(&text);
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
                                let piece: String = chars
                                    [s.start.min(chars.len())..s.end.min(chars.len())]
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
                    ui.add(egui::Label::new(job).wrap());
                });
            }
            Block::Quote(job) => {
                ui.horizontal_top(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(4.0, 20.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, style.quote_bar);
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(job).wrap());
                });
                ui.add_space(4.0);
            }
            Block::TableRow { cells, header } => {
                ui.horizontal(|ui| {
                    for c in cells {
                        let mut job = c;
                        if header {
                            for s in &mut job.sections {
                                s.format.color = style.text;
                            }
                        }
                        ui.add_sized(
                            [ui.available_width() / 4.0_f32.max(1.0), 18.0],
                            egui::Label::new(job).truncate(),
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

fn parse(text: &str, style: &MarkdownStyle, _width: f32) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(text, opts);
    let mut blocks = Vec::new();
    let mut job = egui::text::LayoutJob::default();
    let mut inline = Inline::default();
    let mut heading: Option<HeadingLevel> = None;
    let mut list_depth = 0usize;
    let mut list_counters: Vec<Option<u64>> = Vec::new();
    let mut in_item = false;
    let mut in_quote = false;
    let mut code: Option<(String, String)> = None;
    let mut table_cells: Vec<egui::text::LayoutJob> = Vec::new();
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
                    } else if !job.text.is_empty() {
                        blocks.push(Block::Paragraph(std::mem::take(&mut job)));
                    }
                }
                TagEnd::Heading(level) => {
                    blocks.push(Block::Heading(level, std::mem::take(&mut job)));
                    heading = None;
                }
                TagEnd::BlockQuote(_) => {
                    if !job.text.is_empty() {
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
                } else {
                    let f = fmt(
                        style,
                        &inline,
                        size_now(heading),
                        heading.is_some() || (in_table && in_table_header),
                    );
                    job.append(&t, 0.0, f);
                }
            }
            Event::Code(t) => {
                inline.code += 1;
                let f = fmt(style, &inline, size_now(heading), false);
                job.append(&t, 0.0, f);
                inline.code -= 1;
            }
            Event::SoftBreak => {
                let f = fmt(style, &inline, size_now(heading), false);
                job.append(" ", 0.0, f);
            }
            Event::HardBreak => {
                let f = fmt(style, &inline, size_now(heading), false);
                job.append("\n", 0.0, f);
            }
            Event::Rule => blocks.push(Block::Rule),
            Event::TaskListMarker(done) => {
                let f = fmt(style, &inline, base, false);
                job.append(if done { "☑ " } else { "☐ " }, 0.0, f);
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                let f = egui::TextFormat {
                    font_id: egui::FontId::monospace(base - 1.0),
                    color: style.muted,
                    ..Default::default()
                };
                job.append(&h, 0.0, f);
            }
            _ => {}
        }
    }
    if !job.text.is_empty() {
        blocks.push(Block::Paragraph(job));
    }
    blocks
}

#[cfg(test)]
mod tests {
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
