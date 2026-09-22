//! An order-preserving document model.
//!
//! A world blueprint is read, rewritten and written back out, and the file it
//! writes is checked into git and hashed by the determinism corpus. Key order
//! therefore has to survive the round trip: a map that sorted its keys would
//! reorder every one of the reference world's ninety-three services the first
//! time anything touched it, and turn a one-line change into a whole-file diff.
//! `serde_json::Value` sorts, so the blueprint carries its own model instead.
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    /// A number kept as it was written. Every number in this repository's world
    /// files is already in the shortest round-tripping form, so writing the text
    /// back out is exact, and `json::parse` refuses any literal that is not —
    /// rather than quietly reformatting someone's file.
    Number(String),
    String(String),
    List(Vec<Node>),
    Map(Map),
}

impl Node {
    pub fn string(text: impl Into<String>) -> Self {
        Node::String(text.into())
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Node::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[Node]> {
        match self {
            Node::List(items) => Some(items),
            _ => None,
        }
    }
    pub fn as_list_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Node::List(items) => Some(items),
            _ => None,
        }
    }
    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Node::Map(map) => Some(map),
            _ => None,
        }
    }
    pub fn as_map_mut(&mut self) -> Option<&mut Map> {
        match self {
            Node::Map(map) => Some(map),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.as_map()?.get(key)
    }
    /// The scalar as it substitutes into text: a string as itself, a number or a
    /// boolean as written, so `${PORT}` reads `8080` rather than `"8080"`.
    pub fn scalar_text(&self) -> Option<String> {
        match self {
            Node::String(s) => Some(s.clone()),
            Node::Number(n) => Some(n.clone()),
            Node::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
    /// What this is, for an error message.
    pub fn kind(&self) -> &'static str {
        match self {
            Node::Null => "null",
            Node::Bool(_) => "a boolean",
            Node::Number(_) => "a number",
            Node::String(_) => "a string",
            Node::List(_) => "a list",
            Node::Map(_) => "a mapping",
        }
    }
}

/// An insertion-ordered mapping with unique keys.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Map(Vec<(String, Node)>);

impl Map {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }
    /// Set `key`, keeping the position it already held. Returns the old value.
    pub fn insert(&mut self, key: impl Into<String>, value: Node) -> Option<Node> {
        let key = key.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => Some(std::mem::replace(&mut slot.1, value)),
            None => {
                self.0.push((key, value));
                None
            }
        }
    }
    /// Where `key` sits, for an insertion that has to land in its place.
    pub fn position(&self, key: &str) -> Option<usize> {
        self.0.iter().position(|(k, _)| k == key)
    }
    /// Set `key` at `at`, unless it is already present, in which case it keeps
    /// the position it had. A directive that stands in for a key — `copy:` for
    /// the files it seeds — uses this so the resolved document reads in the
    /// order it was written.
    pub fn insert_at(&mut self, at: usize, key: impl Into<String>, value: Node) {
        let key = key.into();
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
            return;
        }
        self.0.insert(at.min(self.0.len()), (key, value));
    }
    pub fn remove(&mut self, key: &str) -> Option<Node> {
        let at = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(at).1)
    }
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Node)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut Node)> {
        self.0.iter_mut().map(|(k, v)| (k.as_str(), v))
    }
    /// Reorder in place by key. Only the seeded-file maps are sorted; everything
    /// else keeps the order its author wrote.
    pub fn sort_by_key_bytes(&mut self) {
        self.0.sort_by(|(a, _), (b, _)| a.cmp(b));
    }
    pub fn into_pairs(self) -> Vec<(String, Node)> {
        self.0
    }
}

impl FromIterator<(String, Node)> for Map {
    fn from_iter<T: IntoIterator<Item = (String, Node)>>(iter: T) -> Self {
        let mut map = Map::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

impl IntoIterator for Map {
    type Item = (String, Node);
    type IntoIter = std::vec::IntoIter<(String, Node)>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl fmt::Display for Node {
    /// The compact form, for error messages and for embedding in generated files.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::json::write(self, crate::json::Format::Compact))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_keeps_the_position_a_key_already_held() {
        let mut map = Map::new();
        map.insert("a", Node::Null);
        map.insert("b", Node::Null);
        map.insert("a", Node::Bool(true));
        assert_eq!(map.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(map.get("a"), Some(&Node::Bool(true)));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn insert_at_lands_in_place_but_never_moves_an_existing_key() {
        let mut map: Map = [("id", Node::Null), ("packages", Node::Null)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        map.insert_at(1, "initial_files", Node::Bool(true));
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            ["id", "initial_files", "packages"]
        );
        map.insert_at(0, "packages", Node::Bool(false));
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            ["id", "initial_files", "packages"]
        );
        map.insert_at(99, "last", Node::Null);
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            ["id", "initial_files", "packages", "last"]
        );
    }

    #[test]
    fn remove_closes_the_gap() {
        let mut map: Map = [("a", Node::Null), ("b", Node::Null), ("c", Node::Null)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        assert_eq!(map.remove("b"), Some(Node::Null));
        assert_eq!(map.keys().collect::<Vec<_>>(), ["a", "c"]);
        assert_eq!(map.remove("b"), None);
    }

    #[test]
    fn scalars_substitute_as_written() {
        assert_eq!(Node::Number("8080".into()).scalar_text().unwrap(), "8080");
        assert_eq!(Node::Bool(false).scalar_text().unwrap(), "false");
        assert_eq!(Node::string("x").scalar_text().unwrap(), "x");
        assert_eq!(Node::Null.scalar_text(), None);
        assert_eq!(Node::List(vec![]).scalar_text(), None);
    }

    #[test]
    fn sorting_is_by_key_bytes() {
        let mut map: Map = ["Documents/b", "Documents/KiCad/a", "notes.txt", "Notes/x"]
            .into_iter()
            .map(|k| (k.to_string(), Node::Null))
            .collect();
        map.sort_by_key_bytes();
        assert_eq!(
            map.keys().collect::<Vec<_>>(),
            ["Documents/KiCad/a", "Documents/b", "Notes/x", "notes.txt"]
        );
    }
}
