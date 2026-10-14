//! HTML export for "Print…": every tool can hand the shell a title and an
//! HTML body (`Tool::printable`), and the shell wraps it in a standalone
//! page with a light/dark switch and a Print button, writes it under the
//! data folder's `tmp/` and opens it in the browser, which prints to paper
//! or PDF. No script runs apart from the page's own two buttons.

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// `<pre>` with escaped text, for console output and plain code.
pub fn pre(text: &str) -> String {
    format!("<pre class=\"hs-pre\">{}</pre>", escape(text))
}

/// A table from a header row and cell rows (all escaped).
pub fn table(headers: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::from("<table class=\"hs-table\"><thead><tr>");
    for h in headers {
        out.push_str(&format!("<th>{}</th>", escape(h)));
    }
    out.push_str("</tr></thead><tbody>");
    for r in rows {
        out.push_str("<tr>");
        for c in r {
            out.push_str(&format!("<td>{}</td>", escape(c)));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

/// A full page around `body`. `dark` picks the initial scheme; the page
/// keeps a Light/Dark switch and a Print button in a bar that is hidden
/// when printing.
pub fn document(title: &str, subtitle: &str, body: &str, dark: bool) -> String {
    let title = escape(title);
    let subtitle = escape(subtitle);
    let scheme = if dark { "dark" } else { "light" };
    format!(
        r##"<!doctype html>
<html lang="en" data-scheme="{scheme}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ --bg:#f6f6f8; --panel:#ffffff; --text:#1e1e24; --muted:#6a6a76; --border:#d4d4dc; --accent:#7200ca; --code:#eeeef2; }}
html[data-scheme="dark"] {{ --bg:#15151a; --panel:#1c1c23; --text:#e6e6ea; --muted:#9a9aa6; --border:#34343f; --accent:#9a4df0; --code:#24242d; }}
* {{ box-sizing:border-box; }}
body {{ margin:0; background:var(--bg); color:var(--text); font:14px/1.5 -apple-system, "Segoe UI", Ubuntu, Roboto, sans-serif; }}
.bar {{ position:sticky; top:0; display:flex; gap:8px; align-items:center; padding:8px 16px; background:var(--panel); border-bottom:1px solid var(--border); }}
.bar .title {{ font-weight:600; margin-right:auto; }}
.bar button {{ padding:6px 12px; border:1px solid var(--border); border-radius:6px; background:var(--bg); color:var(--text); cursor:pointer; }}
.bar button:hover {{ border-color:var(--accent); }}
main {{ max-width:1100px; margin:0 auto; padding:20px 24px 40px; }}
h1 {{ font-size:20px; margin:0 0 2px; }}
.sub {{ color:var(--muted); margin:0 0 18px; }}
.hs-pre, pre {{ background:var(--code); border:1px solid var(--border); border-radius:6px; padding:12px; overflow:auto; font:12.5px/1.45 "JetBrains Mono", Consolas, "Fira Code", monospace; white-space:pre-wrap; word-break:break-word; }}
.hs-table {{ border-collapse:collapse; width:100%; font-size:13px; }}
.hs-table th, .hs-table td {{ border:1px solid var(--border); padding:4px 8px; text-align:left; vertical-align:top; font-family:"JetBrains Mono", Consolas, monospace; }}
.hs-table th {{ background:var(--code); }}
.md img {{ max-width:100%; }}
.md table {{ border-collapse:collapse; }}
.md th, .md td {{ border:1px solid var(--border); padding:4px 8px; }}
.md code {{ background:var(--code); padding:1px 4px; border-radius:3px; }}
.md blockquote {{ border-left:3px solid var(--accent); margin:0; padding:2px 12px; color:var(--muted); }}
@media print {{ .bar {{ display:none; }} body {{ background:#fff; color:#000; }} main {{ max-width:none; padding:0; }} .hs-pre, pre {{ border:none; background:#f3f3f3; }} }}
</style>
</head>
<body>
<div class="bar">
  <span class="title">{title}</span>
  <button onclick="document.documentElement.dataset.scheme='light'">Light</button>
  <button onclick="document.documentElement.dataset.scheme='dark'">Dark</button>
  <button onclick="window.print()">Print…</button>
</div>
<main>
<h1>{title}</h1>
<p class="sub">{subtitle}</p>
{body}
</main>
</body>
</html>
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_and_builds_tables() {
        assert_eq!(
            escape("<a href=\"x\">&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;"
        );
        let t = table(&["id".into()], &[vec!["<1>".into()]]);
        assert!(t.contains("<th>id</th>") && t.contains("<td>&lt;1&gt;</td>"));
        let doc = document("T<", "sub", "<p>b</p>", true);
        assert!(doc.contains("data-scheme=\"dark\"") && doc.contains("<title>T&lt;</title>"));
        assert!(doc.contains("<p>b</p>"));
    }
}
