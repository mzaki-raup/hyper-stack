//! Just enough XML for S3 responses: a tree of elements with text, no
//! namespaces (prefixes are dropped), attributes ignored, the five named
//! entities plus numeric references, CDATA and comments handled.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    /// First child named `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Every child named `name`.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Text of the first child named `name` ("" when absent).
    pub fn text_of(&self, name: &str) -> String {
        self.child(name).map(|c| c.text.clone()).unwrap_or_default()
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(end) = after.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let entity = &after[..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16)
                .ok()
                .and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Parse a document into its root element.
pub fn parse(doc: &str) -> Result<Element, String> {
    let mut stack: Vec<Element> = vec![Element::default()];
    let mut rest = doc;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&unescape(rest));
            }
            break;
        };
        if lt > 0
            && let Some(top) = stack.last_mut()
        {
            top.text.push_str(&unescape(&rest[..lt]));
        }
        rest = &rest[lt..];
        if let Some(r) = rest.strip_prefix("<?") {
            let e = r.find("?>").ok_or("unterminated declaration")?;
            rest = &r[e + 2..];
        } else if let Some(r) = rest.strip_prefix("<!--") {
            let e = r.find("-->").ok_or("unterminated comment")?;
            rest = &r[e + 3..];
        } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let e = r.find("]]>").ok_or("unterminated CDATA")?;
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&r[..e]);
            }
            rest = &r[e + 3..];
        } else if let Some(r) = rest.strip_prefix("<!") {
            let e = r.find('>').ok_or("unterminated doctype")?;
            rest = &r[e + 1..];
        } else if let Some(r) = rest.strip_prefix("</") {
            let e = r.find('>').ok_or("unterminated end tag")?;
            let name = local(r[..e].trim());
            let el = stack.pop().ok_or("unbalanced end tag")?;
            if el.name != name {
                return Err(format!("expected </{}>, found </{name}>", el.name));
            }
            stack
                .last_mut()
                .ok_or("unbalanced end tag")?
                .children
                .push(el);
            rest = &r[e + 1..];
        } else {
            let r = &rest[1..];
            let e = r.find('>').ok_or("unterminated start tag")?;
            let inner = &r[..e];
            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let name = local(inner.split_whitespace().next().unwrap_or("")).to_string();
            if name.is_empty() {
                return Err("empty tag name".into());
            }
            let el = Element {
                name,
                ..Default::default()
            };
            if self_closing {
                stack.last_mut().ok_or("no parent")?.children.push(el);
            } else {
                stack.push(el);
            }
            rest = &r[e + 1..];
        }
    }
    if stack.len() != 1 {
        return Err("unclosed elements".into());
    }
    let mut doc = stack.pop().expect("document");
    doc.children
        .pop()
        .filter(|_| doc.children.is_empty())
        .ok_or_else(|| "expected exactly one root element".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_list_result_with_entities_and_namespaces() {
        let doc = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>b</Name><IsTruncated>false</IsTruncated>
  <Contents><Key>a &amp; b/c&#x2B;d.txt</Key><Size>5</Size></Contents>
  <Contents><Key>x&lt;y</Key><Size>0</Size><Owner/></Contents>
  <CommonPrefixes><Prefix>dir/</Prefix></CommonPrefixes>
  <!-- comment -->
  <s3:Extra><![CDATA[<raw>]]></s3:Extra>
</ListBucketResult>"#;
        let root = parse(doc).unwrap();
        assert_eq!(root.name, "ListBucketResult");
        let keys: Vec<String> = root.all("Contents").map(|c| c.text_of("Key")).collect();
        assert_eq!(keys, vec!["a & b/c+d.txt", "x<y"]);
        assert_eq!(
            root.child("CommonPrefixes").unwrap().text_of("Prefix"),
            "dir/"
        );
        assert_eq!(root.text_of("Extra"), "<raw>");
    }

    #[test]
    fn rejects_broken_documents() {
        assert!(parse("<a><b></a>").is_err());
        assert!(parse("<a>").is_err());
        assert!(parse("<a/><b/>").is_err());
    }

    #[test]
    fn escape_round_trips() {
        let s = r#"a<b>&"c'd"#;
        assert_eq!(unescape(&escape(s)), s);
        assert_eq!(unescape("5 & 6 &unknown; &#169;"), "5 & 6 &unknown; ©");
    }
}
