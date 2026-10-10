//! Fuzzy ranking for the palette and quick-open: subsequence matching with
//! bonuses for word starts, camel humps, consecutive runs and prefix matches.

#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub index: usize,
    pub score: i32,
    /// Byte positions of matched characters (for highlighting).
    pub positions: Vec<usize>,
}

/// Score `query` against `text`; `None` when not every query char appears in order.
pub fn score(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let t: Vec<(usize, char)> = text.char_indices().collect();
    let mut qi = 0;
    let mut score = 0i32;
    let mut positions = Vec::with_capacity(q.len());
    let mut prev_matched = false;
    for (i, (byte, ch)) in t.iter().enumerate() {
        if qi >= q.len() {
            break;
        }
        let lower: Vec<char> = ch.to_lowercase().collect();
        if lower.first() == Some(&q[qi]) {
            let mut s = 10;
            if i == 0 {
                s += 20; // prefix
            } else {
                let prev = t[i - 1].1;
                if !prev.is_alphanumeric() {
                    s += 15; // word start after separator
                } else if ch.is_uppercase() && prev.is_lowercase() {
                    s += 12; // camel hump
                }
            }
            if prev_matched {
                s += 8; // consecutive
            }
            score += s;
            positions.push(*byte);
            qi += 1;
            prev_matched = true;
        } else {
            prev_matched = false;
            score -= 1; // gap penalty
        }
    }
    if qi == q.len() {
        // Shorter texts rank higher on ties.
        score -= (t.len() as i32 - q.len() as i32).max(0) / 4;
        Some((score, positions))
    } else {
        None
    }
}

/// Rank `items` (title, optional subtitle) for `query`, best first.
pub fn rank(query: &str, items: &[(&str, Option<&str>)]) -> Vec<Match> {
    let mut out: Vec<Match> = items
        .iter()
        .enumerate()
        .filter_map(|(index, (title, subtitle))| {
            // Title first, then the subtitle alone, then "subtitle/title" so a
            // path-style query ("tools/editor.md") finds a file shown as
            // title "editor.md" + subtitle "docs/tools".
            let best = score(query, title)
                .or_else(|| {
                    subtitle
                        .and_then(|st| score(query, st))
                        .map(|(s, _)| (s - 30, Vec::new()))
                })
                .or_else(|| {
                    subtitle
                        .and_then(|st| score(query, &format!("{st}/{title}")))
                        .map(|(s, _)| (s - 40, Vec::new()))
                })?;
            Some(Match {
                index,
                score: best.0,
                positions: best.1,
            })
        })
        .collect();
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_style_queries_match_subtitle_plus_title() {
        let items = [("editor.md", Some("docs/tools")), ("README.md", Some(""))];
        let r = rank("tools/editor.md", &items);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].index, 0);
        assert!(rank("nothing/here", &items).is_empty());
    }

    #[test]
    fn prefix_and_word_starts_beat_scattered_matches() {
        let items = [
            ("Close tab", None),
            ("New connection", None),
            ("Switch environment", None),
            ("Settings: Connections", None),
        ];
        let r = rank("conn", &items);
        assert_eq!(r[0].index, 1);
        assert_eq!(r[1].index, 3);
        assert!(r.iter().all(|m| m.index != 0));
    }

    #[test]
    fn abbreviations_match_camel_and_separators() {
        assert!(score("sce", "Switch Current Environment").is_some());
        assert!(score("ssh", "Settings: SSH hosts").is_some());
        assert!(score("zz", "Settings").is_none());
        let (s1, _) = score("set", "Settings").unwrap();
        let (s2, _) = score("set", "Reset layout").unwrap();
        assert!(s1 > s2);
    }

    #[test]
    fn empty_query_keeps_order() {
        let items = [("b", None), ("a", None)];
        let r = rank("", &items);
        assert_eq!(r.iter().map(|m| m.index).collect::<Vec<_>>(), vec![0, 1]);
    }
}
