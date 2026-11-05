//! The folder column's hierarchy (FEAT-0097): the bucket's prefixes as a
//! tree that expands in place. A bucket has no folders to read up front,
//! only delimiter listings one prefix at a time, so the tree learns a
//! node's children when that prefix is listed - by the pane moving into it
//! or by the caret being opened - and asks for what an open node still
//! lacks. Expansion lives in the widget's `TreeState`, outside this type,
//! so a refresh can throw the learned children away and keep what was open.

use std::collections::{BTreeMap, HashSet};

use hs_ui::tree::{TreeIcon, TreeNode};

#[derive(Debug, Default)]
pub struct FolderTree {
    /// Prefix → the child prefixes its listing reported. A prefix that is
    /// absent has not been listed yet.
    children: BTreeMap<String, Vec<String>>,
    /// Prefixes whose listing is on its way, so it is asked for once.
    loading: HashSet<String>,
}

impl FolderTree {
    /// Record what a listing of `prefix` reported. `replace` for the first
    /// page of a listing; a later page adds to it.
    pub fn learn(&mut self, prefix: &str, found: Vec<String>, replace: bool) {
        self.loading.remove(prefix);
        let slot = self.children.entry(prefix.to_string()).or_default();
        if replace {
            slot.clear();
        }
        slot.extend(found);
        slot.sort();
        slot.dedup();
    }

    /// Forget every listing (Refresh, or another bucket); what is open
    /// stays open in the widget's state and is listed again on demand.
    pub fn forget(&mut self) {
        self.children.clear();
        self.loading.clear();
    }

    /// The root and the open prefixes whose children are not known and
    /// not yet asked for, now marked as asked for. The caller lists them.
    /// The root is always wanted: after a Refresh nothing is known, and
    /// the pane may be deep inside the bucket, so its own listing alone
    /// would leave the top of the tree empty.
    pub fn wanted(&mut self, expanded: &HashSet<String>) -> Vec<String> {
        let root = String::new();
        let mut out: Vec<String> = std::iter::once(&root)
            .chain(expanded.iter())
            .filter(|p| !self.children.contains_key(*p) && !self.loading.contains(*p))
            .cloned()
            .collect();
        out.sort();
        for p in &out {
            self.loading.insert(p.clone());
        }
        out
    }

    /// The nodes under `prefix` (`""` for the bucket's root), as deep as
    /// the listings go: a known childless prefix is a leaf, an unlisted
    /// one carries a caret and `children: None` so the widget asks.
    pub fn nodes(&self, prefix: &str, icon: &dyn Fn(&str) -> TreeIcon) -> Vec<TreeNode> {
        let Some(kids) = self.children.get(prefix) else {
            return Vec::new();
        };
        kids.iter()
            .map(|p| {
                let label = name_of(p).to_string();
                let mut node = match self.children.get(p) {
                    Some(c) if c.is_empty() => {
                        TreeNode::leaf(p.clone(), label.clone(), icon(&label))
                    }
                    Some(_) => TreeNode::branch(
                        p.clone(),
                        label.clone(),
                        icon(&label),
                        self.nodes(p, icon),
                    ),
                    None => {
                        let mut n = TreeNode::leaf(p.clone(), label.clone(), icon(&label));
                        n.leaf = false;
                        n
                    }
                };
                node.children = match self.children.get(p) {
                    Some(c) if c.is_empty() => Some(Vec::new()),
                    Some(_) => node.children,
                    None => None,
                };
                node
            })
            .collect()
    }
}

/// The last segment of a prefix: `a/b/c/` → `c`.
pub fn name_of(prefix: &str) -> &str {
    prefix
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(prefix)
}

/// Every prefix above one, nearest last: `a/b/c/` → `a/`, `a/b/`.
pub fn ancestors(prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut acc = String::new();
    let trimmed = prefix.trim_end_matches('/');
    let Some((parents, _)) = trimmed.rsplit_once('/') else {
        return out;
    };
    for seg in parents.split('/') {
        acc.push_str(seg);
        acc.push('/');
        out.push(acc.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hs_ui::tree::TreeState;

    fn glyph(_: &str) -> TreeIcon {
        TreeIcon::Glyph("F".into())
    }

    #[test]
    fn the_hierarchy_builds_lazily_from_the_listings() {
        let mut t = FolderTree::default();
        assert!(t.nodes("", &glyph).is_empty(), "nothing listed yet");
        assert_eq!(
            t.wanted(&HashSet::new()),
            vec![String::new()],
            "the root first"
        );
        t.learn("", vec!["docs/".into(), "src/".into()], true);
        let root = t.nodes("", &glyph);
        assert_eq!(root.len(), 2);
        assert_eq!(root[0].label, "docs");
        assert!(
            root[0].children.is_none() && !root[0].leaf,
            "unlisted: a caret, and ask"
        );
        // Opening `src/` lists it; its children appear in place.
        let mut state = TreeState::default();
        state.expanded.insert("src/".into());
        assert_eq!(t.wanted(&state.expanded), vec!["src/".to_string()]);
        assert!(t.wanted(&state.expanded).is_empty(), "asked once");
        t.learn("src/", vec!["src/lib/".into()], true);
        let root = t.nodes("", &glyph);
        let src = &root[1];
        assert_eq!(src.children.as_ref().map(Vec::len), Some(1));
        assert_eq!(src.children.as_ref().unwrap()[0].id, "src/lib/");
        // A second page adds to the first.
        t.learn("src/", vec!["src/bin/".into()], false);
        assert_eq!(t.nodes("src/", &glyph).len(), 2);
        // A listed prefix with no folders in it is a leaf.
        t.learn("docs/", Vec::new(), true);
        assert!(t.nodes("", &glyph)[0].leaf);
    }

    #[test]
    fn a_refresh_forgets_the_listings_but_not_what_was_open() {
        let mut t = FolderTree::default();
        t.learn("", vec!["a/".into()], true);
        t.learn("a/", vec!["a/b/".into()], true);
        let mut state = TreeState::default();
        state.expanded.insert("a/".into());
        assert!(t.wanted(&state.expanded).is_empty(), "a/ is known");
        t.forget();
        assert!(t.nodes("", &glyph).is_empty());
        // Still open in the widget's state, so it is listed again - and so
        // is the root, whatever the pane is looking at.
        assert!(state.expanded.contains("a/"));
        assert_eq!(
            t.wanted(&state.expanded),
            vec![String::new(), "a/".to_string()]
        );
    }

    #[test]
    fn names_and_ancestors_come_off_the_prefix() {
        assert_eq!(name_of("a/b/c/"), "c");
        assert_eq!(name_of("top/"), "top");
        assert_eq!(ancestors("a/b/c/"), vec!["a/", "a/b/"]);
        assert!(ancestors("a/").is_empty());
        assert!(ancestors("").is_empty());
    }
}
