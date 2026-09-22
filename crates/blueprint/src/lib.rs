//! Resolve a world blueprint into a world definition.
//!
//! A [`cw_protocol::WorldDefinition`] is a hermetic artifact: it is embedded in
//! every native and Wasm build, its bytes are hashed by the determinism corpus,
//! and a snapshot refuses to load into a world whose definition differs. That is
//! what makes it reproducible, and it is also what makes it miserable to write by
//! hand — the reference world is three and a half megabytes of generated JSON.
//!
//! A blueprint is the same document with four source-only conveniences, resolved
//! away before anything runs:
//!
//! * `inputs:` declares the environment variables `${NAME}` may read. Nothing
//!   undeclared is substituted, so a world cannot quietly depend on the machine
//!   that built it.
//! * `include:` merges partial world documents, which is how ninety-three
//!   services live in ninety-three reviewable files instead of one.
//! * `copy:` seeds a computer from a directory of real files, so a seeded home
//!   folder is edited with an editor rather than as JSON string literals.
//! * `place:` gives a service its node, its link and its DNS records from one
//!   block, which is what a hundred links and two hundred records collapse into.
//!
//! Resolution is pure: it reads through a [`Files`] provider and an explicit map
//! of environment values, and touches neither the filesystem nor the environment
//! itself. `cw-world` is the binary that supplies both.
pub mod copy;
pub mod inputs;
pub mod json;
pub mod merge;
pub mod node;
pub mod place;
pub mod schema;
pub mod yaml;

use node::{Map, Node};
use std::collections::{BTreeMap, BTreeSet};

/// A blueprint that could not be resolved, named by the file it was found in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub source: Option<String>,
    pub message: String,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            source: None,
            message: message.into(),
        }
    }
    pub fn at(source: &str, message: impl Into<String>) -> Self {
        Self {
            source: Some(source.to_string()),
            message: message.into(),
        }
    }
    /// Name the file, unless the error already names one closer to the problem.
    pub fn within(mut self, source: &str) -> Self {
        self.source.get_or_insert_with(|| source.to_string());
        self
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            Some(source) => write!(f, "{source}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for Error {}

impl From<cw_protocol::SimError> for Error {
    fn from(e: cw_protocol::SimError) -> Self {
        Self::new(e.to_string())
    }
}

/// One entry of a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
}

/// The files a blueprint may read. Paths are relative to the blueprint's root
/// directory and always `/`-separated; a provider refuses anything that climbs
/// out of it, so a world can only be built from the files that travel with it.
pub trait Files {
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
    /// The entries of `path`, sorted by name. An absent directory is an error;
    /// callers that tolerate absence check [`Files::exists`] first.
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>, String>;
    fn exists(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
}

/// A resolved world, and what it took to build it.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The world definition, with its key order intact.
    pub world: Node,
    /// The value every declared input resolved to, for the build log. Secret
    /// inputs are absent: they may not reach the artifact and are not reported.
    pub inputs: BTreeMap<String, String>,
    /// Services whose body came from an included fragment. The writer puts these
    /// on one line each — the file the service is edited in is where it reads
    /// properly, and expanding all ninety-three would triple the world file.
    pub compact_services: BTreeSet<String>,
    /// Every file the blueprint read, sorted. A build that cannot say what it
    /// depended on cannot be checked for staleness.
    pub read: BTreeSet<String>,
}

impl Resolved {
    /// The world file's bytes, in the shape the repository checks in.
    pub fn to_world_json(&self) -> String {
        // Only the services array is ever compacted, and only those that came
        // from a file of their own. The predicate is positional, so which index
        // is compact is worked out once against the document.
        let services = self
            .world
            .get("services")
            .and_then(Node::as_list)
            .unwrap_or(&[]);
        let compacted: Vec<bool> = services
            .iter()
            .map(|s| {
                s.get("id")
                    .and_then(Node::as_str)
                    .is_some_and(|id| self.compact_services.contains(id))
            })
            .collect();
        let select = |path: &[json::Step<'_>]| match path {
            [json::Step::Key("services"), json::Step::Index(i)] => {
                compacted.get(*i).copied().unwrap_or(false)
            }
            _ => false,
        };
        format!(
            "{}\n",
            json::write(&self.world, json::Format::PrettyExcept(&select))
        )
    }

    /// The definition, typed and validated by the engine's own rules.
    pub fn definition(&self) -> Result<cw_protocol::WorldDefinition, Error> {
        let json = json::write(&self.world, json::Format::Compact);
        cw_protocol::WorldDefinition::from_json(&json).map_err(Error::from)
    }
}

/// The keys the blueprint layer adds to a world document, and strips again.
const SOURCE_ONLY: &[&str] = &["inputs", "include", "extends", "defaults", "limits"];
/// Every key a world document may carry once the source-only ones are gone.
const WORLD_KEYS: &[&str] = &[
    "schema_version",
    "id",
    "profiles",
    "computers",
    "network",
    "services",
    "metadata",
];

/// Refuse a key nobody will read. A blueprint that silently drops a misspelled
/// `intial_files` is worse than no schema at all, and serde would drop it.
pub(crate) fn known_keys(
    map: &Map,
    allowed: &[&str],
    what: &str,
    source: &str,
) -> Result<(), Error> {
    for key in map.keys() {
        if !allowed.contains(&key) {
            let near = allowed
                .iter()
                .filter(|a| near(a, key))
                .copied()
                .collect::<Vec<_>>();
            let hint = match near.as_slice() {
                [one] => format!(" (did you mean {one}?)"),
                _ => format!(" (known: {})", allowed.join(", ")),
            };
            return Err(Error::at(
                source,
                format!("{what} has no key {key:?}{hint}"),
            ));
        }
    }
    Ok(())
}

/// Whether two keys are within one edit of each other, for the "did you mean".
fn near(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut edits = 0;
    let (mut i, mut j) = (0, 0);
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        if short.len() == long.len() {
            i += 1;
        }
        j += 1;
    }
    edits + (long.len() - j) + (short.len() - i) <= 1
}

/// Resolve the blueprint at `path` into a world definition.
///
/// `supplied` is what the environment offered; only declared inputs are read
/// from it, and an undeclared `${NAME}` is an error rather than an empty string.
pub fn resolve(
    path: &str,
    files: &dyn Files,
    supplied: &BTreeMap<String, String>,
) -> Result<Resolved, Error> {
    let mut read = BTreeSet::new();
    // The root blueprint is read once for its `inputs:` alone: a fragment may use
    // `${NAME}`, but only the file the build was pointed at says what the names
    // are, so one file tells you everything a world needs to be built.
    let root_bytes = files.read(path).map_err(|e| Error::at(path, e))?;
    let declared = match merge::parse(path, &root_bytes)?.get("inputs") {
        Some(value) => inputs::parse(value, path)?,
        None => inputs::Inputs::new(),
    };
    let values = inputs::resolve(&declared, supplied)?;

    let (mut document, compact_services, secrets) = {
        let mut interp = inputs::Interpolator::new(&declared, &values);
        let loaded = merge::load(path, files, &mut read, &mut interp)?;
        (
            loaded.document,
            loaded.fragment_services,
            std::mem::take(&mut interp.used),
        )
    };
    if !secrets.is_empty() {
        return Err(Error::at(
            path,
            format!(
                "secret input(s) {} would be written into the resolved world; a secret may only \
                 appear in a source path such as a `copy.from` or an `include` entry",
                secrets.into_iter().collect::<Vec<_>>().join(", ")
            ),
        ));
    }

    place::sort_dns(&mut document);
    let limits = document.as_map_mut().and_then(|m| m.remove("limits"));
    copy::seed(&mut document, files, limits.as_ref(), path, &mut read)?;

    {
        let Some(map) = document.as_map_mut() else {
            return Err(Error::at(path, "a blueprint is a mapping"));
        };
        for key in SOURCE_ONLY {
            map.remove(key);
        }
        known_keys(map, WORLD_KEYS, "the world", path)?;
        // A world file with no `schema_version` is a world file that will be read
        // by whatever version happens to be current, so one is always written.
        if !map.contains_key("schema_version") {
            let mut with_version = Map::new();
            with_version.insert("schema_version", Node::Number("1".into()));
            for (k, v) in std::mem::take(map) {
                with_version.insert(k, v);
            }
            *map = with_version;
        }
    }
    schema::check(&document, path)?;

    let resolved = Resolved {
        world: document,
        inputs: values
            .into_iter()
            .filter(|(name, _)| !declared[name].secret)
            .collect(),
        compact_services,
        read,
    };
    // The engine's own validation is the last word: duplicate identities, unknown
    // profiles, address collisions, listener collisions and DNS shape. Failing
    // here rather than at `World::new` is the whole point of a build step.
    resolved.definition()?;
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_key_names_the_one_it_was_probably_meant_to_be() {
        let mut map = Map::new();
        map.insert("intial_files", Node::Null);
        let error = known_keys(&map, &["initial_files", "packages"], "a computer", "w.yml")
            .unwrap_err()
            .to_string();
        assert!(error.contains("did you mean initial_files?"), "{error}");
    }

    #[test]
    fn an_unknown_key_that_is_nothing_like_one_lists_them_all() {
        let mut map = Map::new();
        map.insert("nonsense", Node::Null);
        let error = known_keys(&map, &["initial_files", "packages"], "a computer", "w.yml")
            .unwrap_err()
            .to_string();
        assert!(error.contains("known: initial_files, packages"), "{error}");
    }

    #[test]
    fn a_known_key_passes() {
        let mut map = Map::new();
        map.insert("packages", Node::Null);
        assert!(known_keys(&map, &["initial_files", "packages"], "a computer", "w.yml").is_ok());
    }

    #[test]
    fn nearness_is_one_edit() {
        assert!(near("initial_files", "intial_files"));
        assert!(near("packages", "package"));
        assert!(near("node", "nodes"));
        assert!(!near("packages", "profiles"));
        assert!(!near("a", "abc"));
    }

    #[test]
    fn an_error_names_the_file_it_was_found_in() {
        assert_eq!(Error::at("w.yml", "bad").to_string(), "w.yml: bad");
        assert_eq!(Error::new("bad").within("w.yml").to_string(), "w.yml: bad");
        assert_eq!(
            Error::at("a.yml", "bad").within("w.yml").to_string(),
            "a.yml: bad"
        );
    }
}
