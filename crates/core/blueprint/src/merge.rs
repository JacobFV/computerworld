//! Loading a blueprint and the fragments it is composed of.
//!
//! Three directives compose documents, and they all obey one rule: a later
//! contribution replaces an earlier one, in the position the earlier one held.
//! Position matters because the world file is checked in — appending a service
//! rather than replacing it in place would reorder ninety-two others.
//!
//! * `extends: <path>` loads that blueprint first and merges this one over it.
//! * `include: [<path>, ...]` merges each fragment after this document's own
//!   declarations, so what a blueprint states itself comes first.
//! * `{from_file: <path>}` anywhere in the tree becomes that file's contents,
//!   which is how a generated index is data rather than a pipeline stage.
use crate::inputs::Interpolator;
use crate::node::{Map, Node};
use crate::{json, place, yaml, Error, Files};
use std::collections::BTreeSet;

/// A blueprint and the fragments it pulled in.
pub struct Loaded {
    pub document: Node,
    /// Services whose body came from a file other than the root blueprint.
    pub fragment_services: BTreeSet<String>,
}

/// Parse one file by extension. YAML is a superset of JSON, but the stricter
/// parser gives better errors on a `.json` file, and refuses the non-canonical
/// numbers that would not survive a round trip.
pub fn parse(path: &str, bytes: &[u8]) -> Result<Node, Error> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| Error::at(path, format!("is not valid UTF-8: {e}")))?;
    if path.ends_with(".json") {
        json::parse(text).map_err(|e| Error::at(path, e.to_string()))
    } else {
        yaml::parse(text).map_err(|e| Error::at(path, e.to_string()))
    }
}

/// Load the blueprint at `path` with everything it composes resolved.
///
/// Each file is substituted, merged and placed as it arrives, rather than in
/// three passes over the finished document. That ordering is what lets a
/// fragment's own network declarations sit among the ones its services generate
/// — the world file records the order, and a rebuild has to reproduce it.
pub fn load(
    path: &str,
    files: &dyn Files,
    read: &mut BTreeSet<String>,
    interp: &mut Interpolator<'_>,
) -> Result<Loaded, Error> {
    let mut seen = Vec::new();
    let mut fragment_services = BTreeSet::new();
    let document = load_one(
        path,
        files,
        read,
        &mut seen,
        &mut fragment_services,
        interp,
        true,
        true,
    )?;
    Ok(Loaded {
        document,
        fragment_services,
    })
}

#[allow(clippy::too_many_arguments)]
fn load_one(
    path: &str,
    files: &dyn Files,
    read: &mut BTreeSet<String>,
    stack: &mut Vec<String>,
    fragment_services: &mut BTreeSet<String>,
    interp: &mut Interpolator<'_>,
    root: bool,
    // `top` is the document being built, as opposed to one composed into it. Only
    // the outermost document places its services: a fragment has not seen the
    // blueprint's `defaults:` yet, so placing there would read none of them.
    top: bool,
) -> Result<Node, Error> {
    if stack.iter().any(|p| p == path) {
        return Err(Error::at(
            path,
            format!("is included by itself: {} -> {path}", stack.join(" -> ")),
        ));
    }
    stack.push(path.to_string());
    read.insert(path.to_string());
    let bytes = files.read(path).map_err(|e| Error::at(path, e))?;
    let mut own = parse(path, &bytes)?;
    // Inputs substitute into what the file itself says, before a `from_file`
    // brings in data — a generated index is data, not a place for `${NAME}`.
    interp.walk(&mut own, "").map_err(|e| e.within(path))?;
    substitute_files(&mut own, files, read, path)?;
    if !root {
        own = as_world_fragment(own, path)?;
    }

    if !root {
        for id in service_ids(&own) {
            fragment_services.insert(id);
        }
    }

    let (extends, includes) = match own.as_map_mut() {
        Some(map) => (map.remove("extends"), map.remove("include")),
        None => return Err(Error::at(path, "a blueprint is a mapping")),
    };

    let mut document = match extends {
        None | Some(Node::Null) => Node::Map(Map::new()),
        Some(Node::String(base)) => {
            let base = resolve_path(&base, path)?;
            load_one(
                &base,
                files,
                read,
                stack,
                fragment_services,
                interp,
                true,
                false,
            )?
        }
        Some(other) => {
            return Err(Error::at(
                path,
                format!("extends takes one path, not {}", other.kind()),
            ))
        }
    };
    merge(&mut document, own, &mut String::new(), path)?;
    if top {
        place::expand(&mut document, path)?;
    }

    for pattern in include_patterns(includes, path)? {
        for fragment in expand(&pattern, files, path)? {
            let loaded = load_one(
                &fragment,
                files,
                read,
                stack,
                fragment_services,
                interp,
                false,
                false,
            )?;
            merge(&mut document, loaded, &mut String::new(), &fragment)?;
            // Placing now, rather than once at the end, is what keeps this
            // fragment's nodes and links next to the ones it generated.
            if top {
                place::expand(&mut document, &fragment)?;
            }
        }
    }
    stack.pop();
    Ok(document)
}

/// A fragment may be a whole world document or one service on its own. A service
/// is recognised by its `kind`, which no world document has — so a directory of
/// service files needs no wrapper object repeated ninety-three times.
fn as_world_fragment(node: Node, path: &str) -> Result<Node, Error> {
    let Some(map) = node.as_map() else {
        return Err(Error::at(path, "a fragment is a mapping"));
    };
    if !map.contains_key("kind") {
        return Ok(node);
    }
    for directive in ["include", "extends"] {
        if map.contains_key(directive) {
            return Err(Error::at(
                path,
                format!("a service fragment cannot {directive}"),
            ));
        }
    }
    let mut world = Map::new();
    world.insert("services", Node::List(vec![node]));
    Ok(Node::Map(world))
}

fn service_ids(document: &Node) -> Vec<String> {
    document
        .get("services")
        .and_then(Node::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|s| s.get("id").and_then(Node::as_str).map(str::to_string))
        .collect()
}

fn include_patterns(includes: Option<Node>, source: &str) -> Result<Vec<String>, Error> {
    match includes {
        None | Some(Node::Null) => Ok(vec![]),
        Some(Node::List(items)) => items
            .iter()
            .map(|item| match item.as_str() {
                Some(text) => resolve_path(text, source),
                None => Err(Error::at(source, "every include entry is a path")),
            })
            .collect(),
        Some(Node::String(one)) => Ok(vec![resolve_path(&one, source)?]),
        Some(other) => Err(Error::at(
            source,
            format!("include takes a list of paths, not {}", other.kind()),
        )),
    }
}

/// Paths are relative to the blueprint's root directory, never to the file that
/// names them and never to anywhere outside — a world that could reach up out of
/// its own directory would build differently on a different machine.
pub fn resolve_path(path: &str, source: &str) -> Result<String, Error> {
    if path.starts_with('/') || path.contains('\\') {
        return Err(Error::at(
            source,
            format!("{path:?} must be a relative, /-separated path"),
        ));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                return Err(Error::at(
                    source,
                    format!("{path:?} climbs out of the blueprint's directory"),
                ))
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return Err(Error::at(source, "an empty path"));
    }
    Ok(parts.join("/"))
}

/// Expand a pattern that may contain `*` (within one segment) or `**` (across
/// segments). A pattern with no wildcard is itself. Matches come back sorted, so
/// a glob builds the same world twice.
pub fn expand(pattern: &str, files: &dyn Files, source: &str) -> Result<Vec<String>, Error> {
    if !pattern.contains('*') {
        if !files.exists(pattern) {
            return Err(Error::at(source, format!("{pattern} does not exist")));
        }
        return Ok(vec![pattern.to_string()]);
    }
    // Walk from the deepest wildcard-free prefix, so a glob does not list the
    // whole tree to find `sites/*.json`.
    let base = pattern
        .rsplit_once('/')
        .map(|(dir, _)| dir)
        .filter(|dir| !dir.contains('*'))
        .unwrap_or("");
    let mut found = Vec::new();
    walk(base, files, &mut |path, directory| {
        if !directory && matches(pattern, path) {
            found.push(path.to_string());
        }
    })?;
    if found.is_empty() {
        return Err(Error::at(source, format!("{pattern} matched no files")));
    }
    found.sort();
    Ok(found)
}

/// Every path under `dir`, depth first and sorted, `dir` itself excluded.
pub fn walk(dir: &str, files: &dyn Files, visit: &mut dyn FnMut(&str, bool)) -> Result<(), Error> {
    if !dir.is_empty() && !files.is_dir(dir) {
        return Err(Error::new(format!("{dir} is not a directory")));
    }
    let entries = files.list_dir(dir).map_err(|e| Error::at(dir, e))?;
    for entry in entries {
        let path = if dir.is_empty() {
            entry.name.clone()
        } else {
            format!("{dir}/{}", entry.name)
        };
        visit(&path, entry.directory);
        if entry.directory {
            walk(&path, files, visit)?;
        }
    }
    Ok(())
}

/// Glob matching: `*` stops at a `/`, `**` does not, `?` is one character.
pub fn matches(pattern: &str, path: &str) -> bool {
    fn go(p: &[u8], s: &[u8]) -> bool {
        if p.is_empty() {
            return s.is_empty();
        }
        if p.starts_with(b"**") {
            let rest = &p[2..];
            let rest = rest.strip_prefix(b"/").unwrap_or(rest);
            // `**` may stand for nothing at all, so both the empty match and every
            // suffix are tried.
            if go(rest, s) {
                return true;
            }
            for i in 0..s.len() {
                if go(rest, &s[i + 1..]) || (s[i] == b'/' && go(&p[2..], &s[i..])) {
                    return true;
                }
            }
            return false;
        }
        if p[0] == b'*' {
            for i in 0..=s.len() {
                if s[..i].contains(&b'/') {
                    break;
                }
                if go(&p[1..], &s[i..]) {
                    return true;
                }
            }
            return false;
        }
        if s.is_empty() {
            return false;
        }
        if p[0] == b'?' || p[0] == s[0] {
            return go(&p[1..], &s[1..]);
        }
        false
    }
    go(pattern.as_bytes(), path.as_bytes())
}

/// Replace every `{from_file: <path>}` with the contents of that file.
fn substitute_files(
    node: &mut Node,
    files: &dyn Files,
    read: &mut BTreeSet<String>,
    source: &str,
) -> Result<(), Error> {
    match node {
        Node::Map(map) if map.len() == 1 && map.contains_key("from_file") => {
            let Some(path) = map.get("from_file").and_then(Node::as_str) else {
                return Err(Error::at(source, "from_file takes one path"));
            };
            let path = resolve_path(path, source)?;
            read.insert(path.clone());
            let bytes = files.read(&path).map_err(|e| Error::at(&path, e))?;
            *node = parse(&path, &bytes)?;
            // A loaded file may itself be a blueprint fragment full of the same
            // directive, so the replacement is walked too.
            substitute_files(node, files, read, &path)
        }
        Node::Map(map) => {
            for (_, value) in map.iter_mut() {
                substitute_files(value, files, read, source)?;
            }
            Ok(())
        }
        Node::List(items) => {
            for item in items {
                substitute_files(item, files, read, source)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// What identifies an item of a keyed list, so an overlay lands on the right one.
/// A list that is not keyed is replaced wholesale — `installed_apps: [terminal]`
/// means those applications, not those on top of whatever was there.
fn identity(path: &str, item: &Node) -> Option<String> {
    let field = match path {
        "profiles" | "computers" | "services" | "network.nodes" => "id",
        "network.dns" => "name",
        "network.links" | "network.routes" => {
            let from = item.get("from")?.as_str()?;
            let to = item.get("to")?.as_str()?;
            return Some(format!("{from}\u{0}{to}"));
        }
        _ => return None,
    };
    let value = item.get(field)?.as_str()?;
    Some(if field == "name" {
        value.to_ascii_lowercase()
    } else {
        value.to_string()
    })
}

/// Merge `overlay` onto `base`. Later wins; positions are kept.
pub fn merge(base: &mut Node, overlay: Node, path: &mut String, source: &str) -> Result<(), Error> {
    match (base, overlay) {
        (Node::Map(into), Node::Map(from)) => {
            for (key, value) in from {
                match into.get_mut(&key) {
                    Some(existing) => {
                        let was = path.len();
                        if !path.is_empty() {
                            path.push('.');
                        }
                        path.push_str(&key);
                        let result = merge(existing, value, path, source);
                        path.truncate(was);
                        result?;
                    }
                    None => {
                        into.insert(key, value);
                    }
                }
            }
        }
        (Node::List(into), Node::List(from)) => {
            let keyed = from
                .first()
                .is_some_and(|item| identity(path, item).is_some());
            if !keyed {
                *into = from;
                return Ok(());
            }
            for item in from {
                let Some(id) = identity(path, &item) else {
                    return Err(Error::at(
                        source,
                        format!("an entry of {path} has no identifying field"),
                    ));
                };
                match into
                    .iter_mut()
                    .find(|existing| identity(path, existing).as_ref() == Some(&id))
                {
                    Some(existing) => *existing = item,
                    None => into.push(item),
                }
            }
        }
        (slot, value) => *slot = value,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Node {
        yaml::parse(text).unwrap()
    }

    fn merged(base: &str, overlay: &str) -> Node {
        let mut base = doc(base);
        merge(&mut base, doc(overlay), &mut String::new(), "t.yml").unwrap();
        base
    }

    #[test]
    fn a_later_service_replaces_an_earlier_one_in_place() {
        let world = merged(
            "services:\n  - id: a\n    kind: old\n  - id: b\n    kind: b\n",
            "services:\n  - id: a\n    kind: new\n",
        );
        let services = world.get("services").unwrap().as_list().unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].get("kind").unwrap().as_str(), Some("new"));
        assert_eq!(services[1].get("id").unwrap().as_str(), Some("b"));
    }

    #[test]
    fn a_new_service_is_appended() {
        let world = merged("services:\n  - id: a\n", "services:\n  - id: b\n");
        let ids: Vec<_> = world
            .get("services")
            .unwrap()
            .as_list()
            .unwrap()
            .iter()
            .map(|s| s.get("id").unwrap().as_str().unwrap())
            .collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn an_unkeyed_list_is_replaced_not_appended() {
        let world = merged(
            "computers:\n  - id: a\n    installed_apps: [terminal, browser]\n",
            "computers:\n  - id: a\n    installed_apps: [editor]\n",
        );
        let apps = world.get("computers").unwrap().as_list().unwrap()[0]
            .get("installed_apps")
            .unwrap();
        assert_eq!(apps.as_list().unwrap().len(), 1);
    }

    #[test]
    fn links_are_keyed_on_their_directed_pair() {
        let world = merged(
            "network:\n  links:\n    - {from: a, to: b, latency_us: 1}\n",
            "network:\n  links:\n    - {from: a, to: b, latency_us: 2}\n    - {from: b, to: a}\n",
        );
        let links = world
            .get("network")
            .unwrap()
            .get("links")
            .unwrap()
            .as_list()
            .unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(
            links[0].get("latency_us").unwrap(),
            &Node::Number("2".into())
        );
    }

    #[test]
    fn dns_is_keyed_case_insensitively() {
        let world = merged(
            "network:\n  dns:\n    - {name: Wiki.internal, address: 10.0.0.1}\n",
            "network:\n  dns:\n    - {name: wiki.internal, address: 10.0.0.2}\n",
        );
        let dns = world
            .get("network")
            .unwrap()
            .get("dns")
            .unwrap()
            .as_list()
            .unwrap();
        assert_eq!(dns.len(), 1);
        assert_eq!(dns[0].get("address").unwrap().as_str(), Some("10.0.0.2"));
    }

    #[test]
    fn maps_merge_key_by_key_and_keep_position() {
        let world = merged("metadata:\n  a: 1\n  b: 2\n", "metadata:\n  b: 3\n  c: 4\n");
        let keys: Vec<_> = world
            .get("metadata")
            .unwrap()
            .as_map()
            .unwrap()
            .keys()
            .collect();
        assert_eq!(keys, ["a", "b", "c"]);
    }

    #[test]
    fn a_path_may_not_climb_out_of_the_blueprint() {
        assert!(resolve_path("../secrets.yml", "w.yml").is_err());
        assert!(resolve_path("/etc/passwd", "w.yml").is_err());
        assert_eq!(
            resolve_path("./sites/a.json", "w.yml").unwrap(),
            "sites/a.json"
        );
    }

    #[test]
    fn globs_stop_at_a_slash_unless_doubled() {
        assert!(matches("sites/*.json", "sites/a.json"));
        assert!(!matches("sites/*.json", "sites/nested/a.json"));
        assert!(matches("sites/**/*.json", "sites/nested/a.json"));
        assert!(matches("sites/**", "sites/a.json"));
        assert!(matches("home/**/*.txt", "home/a.txt"));
        assert!(!matches("*.json", "a.yml"));
        assert!(matches("a?c", "abc"));
    }
}
