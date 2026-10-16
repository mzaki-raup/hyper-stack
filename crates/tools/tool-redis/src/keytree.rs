//! The key tree of docs/tools/redis.md §4: keys arrive from `SCAN` in no
//! order and are grouped client-side on a delimiter, so `session:1042:profile`
//! sits under `session` › `1042`. Grouping is pure bookkeeping over the keys
//! discovered so far; a group shows `≈` until its scan has finished.

use std::collections::BTreeMap;

use crate::client::KeyType;

/// One row of the flattened, visible tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Full path of the node: the key for a leaf, the prefix for a group.
    pub path: String,
    /// The segment shown, without the parent prefix.
    pub label: String,
    pub depth: usize,
    /// `None` for a group; the key's type for a leaf (once the scan knows it).
    pub kind: Option<KeyType>,
    pub is_group: bool,
    /// Keys discovered under a group, itself included for a leaf.
    pub count: usize,
    pub expanded: bool,
}

/// The keys found so far, grouped on a delimiter.
#[derive(Debug, Default, Clone)]
pub struct KeyTree {
    delimiter: String,
    root: Node,
    len: usize,
}

#[derive(Debug, Default, Clone)]
struct Node {
    children: BTreeMap<String, Node>,
    /// Set when a key ends exactly here.
    kind: Option<Option<KeyType>>,
    count: usize,
}

impl KeyTree {
    pub fn new(delimiter: &str) -> KeyTree {
        KeyTree {
            // An empty delimiter means a flat list, never a split on "".
            delimiter: delimiter.to_string(),
            root: Node::default(),
            len: 0,
        }
    }

    pub fn delimiter(&self) -> &str {
        &self.delimiter
    }

    /// Keys discovered so far.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.root = Node::default();
        self.len = 0;
    }

    /// Add one key. Adding the same key twice counts once.
    pub fn insert(&mut self, key: &str, kind: Option<KeyType>) {
        let segments = self.split(key);
        let mut node = &mut self.root;
        let mut fresh = true;
        for (i, seg) in segments.iter().enumerate() {
            let last = i + 1 == segments.len();
            node = node.children.entry(seg.clone()).or_default();
            if last {
                fresh = node.kind.is_none();
                node.kind = Some(kind.clone());
            }
        }
        if !fresh {
            return;
        }
        self.len += 1;
        // Walk again to bump the counts only for a key that is new.
        let mut node = &mut self.root;
        node.count += 1;
        for seg in &segments {
            node = node.children.get_mut(seg).expect("just inserted");
            node.count += 1;
        }
    }

    fn split(&self, key: &str) -> Vec<String> {
        if self.delimiter.is_empty() {
            return vec![key.to_string()];
        }
        key.split(&self.delimiter).map(str::to_string).collect()
    }

    /// Is this key already in the tree?
    pub fn contains(&self, key: &str) -> bool {
        let mut node = &self.root;
        for seg in self.split(key) {
            match node.children.get(&seg) {
                Some(n) => node = n,
                None => return false,
            }
        }
        node.kind.is_some()
    }

    /// Rows to draw, depth-first, with closed groups hiding their children.
    /// `open` holds the paths of expanded groups.
    pub fn rows(&self, open: &dyn Fn(&str) -> bool) -> Vec<Row> {
        let mut out = Vec::new();
        self.walk(&self.root, "", 0, open, &mut out);
        out
    }

    fn walk(
        &self,
        node: &Node,
        prefix: &str,
        depth: usize,
        open: &dyn Fn(&str) -> bool,
        out: &mut Vec<Row>,
    ) {
        for (label, child) in &node.children {
            let path = if prefix.is_empty() {
                label.clone()
            } else {
                format!("{prefix}{}{label}", self.delimiter)
            };
            // A node can be both: `a` exists and `a:b` exists too.
            let is_group = !child.children.is_empty();
            let expanded = is_group && open(&path);
            out.push(Row {
                label: label.clone(),
                depth,
                kind: child.kind.clone().flatten(),
                is_group,
                count: child.count,
                expanded,
                path: path.clone(),
            });
            if expanded {
                self.walk(child, &path, depth + 1, open, out);
            }
        }
    }

    /// Every key under a prefix, for "delete all under this prefix".
    pub fn keys_under(&self, prefix: &str) -> Vec<String> {
        let mut node = &self.root;
        for seg in self.split(prefix) {
            match node.children.get(&seg) {
                Some(n) => node = n,
                None => return Vec::new(),
            }
        }
        let mut out = Vec::new();
        collect(node, prefix, &self.delimiter, &mut out);
        out
    }

    /// Record the type of a key the scan already found.
    pub fn set_kind(&mut self, key: &str, kind: KeyType) {
        let segments = self.split(key);
        let mut node = &mut self.root;
        for seg in segments {
            match node.children.get_mut(&seg) {
                Some(n) => node = n,
                None => return,
            }
        }
        if node.kind.is_some() {
            node.kind = Some(Some(kind));
        }
    }

    /// Drop one key, after a delete.
    pub fn remove(&mut self, key: &str) {
        if !self.contains(key) {
            return;
        }
        let segments = self.split(key);
        remove_in(&mut self.root, &segments);
        self.root.count = self.root.count.saturating_sub(1);
        self.len -= 1;
    }
}

fn remove_in(node: &mut Node, segments: &[String]) {
    let Some((head, rest)) = segments.split_first() else {
        node.kind = None;
        return;
    };
    let Some(child) = node.children.get_mut(head) else {
        return;
    };
    remove_in(child, rest);
    child.count = child.count.saturating_sub(1);
    // A group with no keys left and no children is gone.
    if child.children.is_empty() && child.kind.is_none() {
        node.children.remove(head);
    }
}

fn collect(node: &Node, prefix: &str, delimiter: &str, out: &mut Vec<String>) {
    if node.kind.is_some() {
        out.push(prefix.to_string());
    }
    for (label, child) in &node.children {
        let path = if prefix.is_empty() {
            label.clone()
        } else {
            format!("{prefix}{delimiter}{label}")
        };
        collect(child, &path, delimiter, out);
    }
}

/// Turn what the user typed in the pattern box into a `SCAN MATCH` glob:
/// a plain prefix becomes `prefix*`, anything with a glob character is used
/// as it is, and an empty box matches every key.
pub fn match_pattern(input: &str) -> String {
    let t = input.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.contains(['*', '?', '[']) {
        t.to_string()
    } else {
        format!("{t}*")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(keys: &[&str]) -> KeyTree {
        let mut t = KeyTree::new(":");
        for k in keys {
            t.insert(k, Some(KeyType::String));
        }
        t
    }

    #[test]
    fn keys_group_on_the_delimiter() {
        let t = tree(&["session:1042:profile", "session:1042:tokens", "cart:77"]);
        assert_eq!(t.len(), 3);
        let rows = t.rows(&|_| false);
        // Two top-level groups, in order, with their discovered counts.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "cart");
        assert_eq!(rows[0].count, 1);
        assert_eq!(rows[1].label, "session");
        assert_eq!(rows[1].count, 2);
        assert!(rows[1].is_group && !rows[1].expanded);
        // Expanding walks down one level at a time.
        let rows = t.rows(&|p| p == "session");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].path, "session:1042");
        assert_eq!(rows[2].depth, 1);
        let rows = t.rows(&|p| p == "session" || p == "session:1042");
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[3].path, "session:1042:profile");
        assert_eq!(rows[3].kind, Some(KeyType::String));
        assert!(!rows[3].is_group);
    }

    #[test]
    fn the_same_key_twice_counts_once() {
        let mut t = tree(&["a:b", "a:b"]);
        assert_eq!(t.len(), 1);
        let rows = t.rows(&|_| true);
        assert_eq!(rows[0].count, 1);
        // And a key that is also a prefix is both a group and a leaf.
        t.insert("a", Some(KeyType::Hash));
        assert_eq!(t.len(), 2);
        let rows = t.rows(&|_| false);
        assert!(rows[0].is_group);
        assert_eq!(rows[0].kind, Some(KeyType::Hash));
        assert_eq!(rows[0].count, 2);
    }

    #[test]
    fn an_empty_delimiter_keeps_the_list_flat() {
        let mut t = KeyTree::new("");
        t.insert("a:b:c", None);
        let rows = t.rows(&|_| true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "a:b:c");
        assert!(!rows[0].is_group);
    }

    #[test]
    fn a_prefix_lists_and_removes_its_keys() {
        let mut t = tree(&["cart:1", "cart:2", "session:9"]);
        assert_eq!(t.keys_under("cart"), vec!["cart:1", "cart:2"]);
        assert!(t.keys_under("nothing").is_empty());
        t.remove("cart:1");
        assert_eq!(t.len(), 2);
        assert!(!t.contains("cart:1"));
        assert_eq!(t.keys_under("cart"), vec!["cart:2"]);
        t.remove("cart:2");
        // The empty group disappears with its last key.
        assert_eq!(t.rows(&|_| false).len(), 1);
        // Removing what is not there changes nothing.
        t.remove("cart:2");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_type_can_be_filled_in_after_the_scan() {
        let mut t = KeyTree::new(":");
        t.insert("a:b", None);
        assert_eq!(t.rows(&|_| true)[1].kind, None);
        t.set_kind("a:b", KeyType::ZSet);
        assert_eq!(t.rows(&|_| true)[1].kind, Some(KeyType::ZSet));
        // Setting the type of a key that is not there is a no-op.
        t.set_kind("nope", KeyType::ZSet);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_prefix_becomes_a_glob_and_a_glob_is_left_alone() {
        assert_eq!(match_pattern("session:"), "session:*");
        assert_eq!(match_pattern(" cart "), "cart*");
        assert_eq!(match_pattern("user:*:tmp"), "user:*:tmp");
        assert_eq!(match_pattern("log[0-9]"), "log[0-9]");
        assert_eq!(match_pattern("  "), "");
    }
}
