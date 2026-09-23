//! Seeding a computer from a directory of real files.
//!
//! `initial_files` is a map of path to contents, which is the right shape for the
//! engine and the wrong shape for a person: a seeded home folder written that way
//! is a JSON string literal with `\n` in it, undiffable and unopenable. `copy:`
//! names a directory instead, and the files in it are ordinary files that an
//! editor can edit and git can diff.
//!
//! The copy happens here, while the world is being built, and not after the
//! machine boots. That is not a detail of convenience. `Runtime::reset` restores
//! the definition's baseline, so files written afterwards vanish at the first
//! reset; snapshot import guards on definition equality, so files outside the
//! definition make two unlike worlds compare equal; and the Wasm build has no
//! filesystem to copy from at all. Folding the bytes into the definition is what
//! keeps all three honest.
use crate::inputs::Interpolator;
use crate::node::Node;
use crate::{known_keys, Error, Files};
use std::collections::{BTreeMap, BTreeSet};

const COPY_KEYS: &[&str] = &["from", "to", "exclude"];
/// Names never seeded, whatever a blueprint says. These are artifacts of the
/// tools that edit the directory, not content of the world.
const NEVER: &[&str] = &[".DS_Store", ".gitkeep", ".git"];
/// What one computer may be seeded with unless its blueprint says otherwise. The
/// world file is embedded in every native and Wasm build, so this is a real cost.
const DEFAULT_BYTES_PER_COMPUTER: u64 = 3 * 1024 * 1024;

/// Apply every computer's `copy:` entries, folding the files into `initial_files`
/// and `initial_binary_files`.
///
/// An entry whose `to:` is `/` mirrors the machine's filesystem: its directories
/// are the machine's absolute paths, `${NAME}` in them substitutes like anywhere
/// else, and on a Windows machine the top-level directory is the drive letter.
/// What lands inside the user's home folder is written home-relative, which is
/// what every other entry already writes.
pub fn seed(
    document: &mut Node,
    files: &dyn Files,
    limits: Option<&Node>,
    source: &str,
    read: &mut BTreeSet<String>,
    interp: &mut Interpolator<'_>,
) -> Result<(), Error> {
    let ceiling = match limits {
        Some(node) => {
            let Some(map) = node.as_map() else {
                return Err(Error::at(source, "limits is a mapping"));
            };
            known_keys(map, &["bytes_per_computer"], "limits", source)?;
            match map.get("bytes_per_computer").and_then(Node::scalar_text) {
                Some(text) => text.parse::<u64>().map_err(|_| {
                    Error::at(
                        source,
                        format!("limits.bytes_per_computer: {text:?} is not a size"),
                    )
                })?,
                None => DEFAULT_BYTES_PER_COMPUTER,
            }
        }
        None => DEFAULT_BYTES_PER_COMPUTER,
    };

    // Each profile's home template and whether its paths start with a drive.
    let profiles: BTreeMap<String, (String, bool)> = document
        .get("profiles")
        .and_then(Node::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| {
            let id = p.get("id")?.as_str()?.to_string();
            let home = p.get("home").and_then(Node::as_str).unwrap_or("");
            let drives = p.get("family").and_then(Node::as_str) == Some("windows");
            Some((id, (home.to_string(), drives)))
        })
        .collect();

    let Some(computers) = document
        .as_map_mut()
        .and_then(|m| m.get_mut("computers"))
        .and_then(Node::as_list_mut)
    else {
        return Ok(());
    };
    for computer in computers.iter_mut() {
        let Some(map) = computer.as_map_mut() else {
            return Err(Error::at(source, "every computer is a mapping"));
        };
        // The seeded files take the place the directive held, so a computer
        // reads in the order it was written rather than growing a key at the end.
        let at = map.position("copy").unwrap_or(map.len());
        let Some(entries) = map.remove("copy") else {
            continue;
        };
        let id = map
            .get("id")
            .and_then(Node::as_str)
            .unwrap_or("<unnamed>")
            .to_string();
        let Some(entries) = entries.as_list() else {
            return Err(Error::at(source, format!("computer {id}: copy is a list")));
        };
        let user = map.get("user").and_then(Node::as_str).unwrap_or("");
        let (home, drives) = map
            .get("profile")
            .and_then(Node::as_str)
            .and_then(|p| profiles.get(p))
            .map(|(home, drives)| (home.replace("{user}", user), *drives))
            .unwrap_or_default();

        // Later entries overlay earlier ones, so `home/all` then `home/ubuntu`
        // reads the way it is written: the profile's own file wins.
        let mut seeded: Vec<(String, Vec<u8>)> = Vec::new();
        let mut bytes = 0u64;
        for entry in entries {
            let Some(entry) = entry.as_map() else {
                return Err(Error::at(
                    source,
                    format!("computer {id}: every copy entry is a mapping"),
                ));
            };
            known_keys(
                entry,
                COPY_KEYS,
                &format!("computer {id}'s copy entry"),
                source,
            )?;
            let Some(from) = entry.get("from").and_then(Node::as_str) else {
                return Err(Error::at(
                    source,
                    format!("computer {id}: a copy entry needs a from"),
                ));
            };
            let from = crate::merge::resolve_path(from, source)?;
            if !files.is_dir(&from) {
                return Err(Error::at(
                    source,
                    format!("computer {id}: {from} is not a directory"),
                ));
            }
            let to = entry.get("to").and_then(Node::as_str).unwrap_or("~");
            let excludes: Vec<&str> = entry
                .get("exclude")
                .and_then(Node::as_list)
                .unwrap_or(&[])
                .iter()
                .filter_map(Node::as_str)
                .collect();

            let mut found = Vec::new();
            crate::merge::walk(&from, files, &mut |path, directory| {
                if !directory {
                    found.push(path.to_string());
                }
            })
            .map_err(|e| e.within(source))?;
            found.sort();
            for path in found {
                let relative = path
                    .strip_prefix(&from)
                    .unwrap_or(&path)
                    .trim_start_matches('/');
                if relative.split('/').any(|part| NEVER.contains(&part)) {
                    continue;
                }
                if excludes.iter().any(|p| crate::merge::matches(p, relative)) {
                    continue;
                }
                read.insert(path.clone());
                let content = files.read(&path).map_err(|e| Error::at(&path, e))?;
                bytes += content.len() as u64;
                let destination = if to == "/" {
                    let relative = interp
                        .text(relative, &format!("computer {id}'s {path}"))
                        .map_err(|e| e.within(source))?
                        .unwrap_or_else(|| relative.to_string());
                    mirror(&relative, &home, drives)
                } else {
                    join(to, relative)
                };
                match seeded.iter_mut().find(|(p, _)| *p == destination) {
                    Some(slot) => slot.1 = content,
                    None => seeded.push((destination, content)),
                }
            }
        }
        if bytes > ceiling {
            return Err(Error::at(
                source,
                format!("computer {id} would be seeded with {bytes} bytes; the limit is {ceiling}"),
            ));
        }

        // A file stated in the blueprint beats one copied in bulk: an exception
        // should not have to be deleted from the directory to be stated.
        let mut text = map
            .get("initial_files")
            .and_then(Node::as_map)
            .cloned()
            .unwrap_or_default();
        let mut binary = map
            .get("initial_binary_files")
            .and_then(Node::as_map)
            .cloned()
            .unwrap_or_default();
        for (path, content) in seeded {
            if text.contains_key(&path) || binary.contains_key(&path) {
                continue;
            }
            match String::from_utf8(content) {
                Ok(valid) => text.insert(path, Node::String(valid)),
                Err(e) => binary.insert(path, Node::String(base64(e.as_bytes()))),
            };
        }
        text.sort_by_key_bytes();
        binary.sort_by_key_bytes();
        if text.is_empty() {
            map.remove("initial_files");
        } else {
            map.insert_at(at, "initial_files", Node::Map(text));
        }
        // An absent key and an empty one are different documents; a computer with
        // no binary seeds should not grow a `{}` nobody wrote.
        if binary.is_empty() {
            map.remove("initial_binary_files");
        } else {
            let after = map.position("initial_files").map_or(at, |i| i + 1);
            map.insert_at(after, "initial_binary_files", Node::Map(binary));
        }
    }
    Ok(())
}

/// A destination path. `~` is the user's home folder, which is where a relative
/// `initial_files` path already resolves, so it is written as a relative path.
fn join(to: &str, relative: &str) -> String {
    let to = to.trim_end_matches('/');
    match to {
        "~" | "" => relative.to_string(),
        _ if to.starts_with("~/") => format!("{}/{relative}", &to[2..]),
        _ => format!("{to}/{relative}"),
    }
}

/// Where a file under a `to: /` directory lands: at its own path from the
/// machine's root, with the first directory a drive on a Windows machine, and
/// home-relative when it is inside `home`.
fn mirror(relative: &str, home: &str, drives: bool) -> String {
    let absolute = match relative.split_once('/') {
        Some((drive, rest)) if drives && drive.len() == 1 => format!("{drive}:/{rest}"),
        _ => format!("/{relative}"),
    };
    match absolute
        .strip_prefix(home)
        .and_then(|r| r.strip_prefix('/'))
    {
        Some(inside) if !home.is_empty() => inside.to_string(),
        _ => absolute,
    }
}

/// Standard base64, the encoding `ComputerDefinition::binary_files` decodes.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{yaml, Entry};
    use std::collections::BTreeMap;

    /// A file provider backed by a map, so the resolver is testable without a disk.
    #[derive(Default)]
    struct Fake(BTreeMap<String, Vec<u8>>);

    impl Fake {
        fn with(files: &[(&str, &[u8])]) -> Self {
            Self(
                files
                    .iter()
                    .map(|(p, c)| (p.to_string(), c.to_vec()))
                    .collect(),
            )
        }
    }

    impl Files for Fake {
        fn read(&self, path: &str) -> Result<Vec<u8>, String> {
            self.0
                .get(path)
                .cloned()
                .ok_or_else(|| "no such file".into())
        }
        fn list_dir(&self, path: &str) -> Result<Vec<Entry>, String> {
            let prefix = if path.is_empty() {
                String::new()
            } else {
                format!("{path}/")
            };
            let mut names = BTreeSet::new();
            for key in self.0.keys() {
                let Some(rest) = key.strip_prefix(&prefix) else {
                    continue;
                };
                match rest.split_once('/') {
                    Some((dir, _)) => names.insert((dir.to_string(), true)),
                    None => names.insert((rest.to_string(), false)),
                };
            }
            Ok(names
                .into_iter()
                .map(|(name, directory)| Entry { name, directory })
                .collect())
        }
        fn exists(&self, path: &str) -> bool {
            self.0.contains_key(path) || self.is_dir(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.0.keys().any(|k| k.starts_with(&format!("{path}/")))
        }
    }

    fn seeded(document: &str, files: &Fake) -> Node {
        let mut document = yaml::parse(document).unwrap();
        let mut read = BTreeSet::new();
        seed(
            &mut document,
            files,
            None,
            "w.yml",
            &mut read,
            &mut no_inputs(),
        )
        .unwrap();
        document
    }

    fn no_inputs() -> Interpolator<'static> {
        static NONE: std::sync::OnceLock<(crate::inputs::Inputs, BTreeMap<String, String>)> =
            std::sync::OnceLock::new();
        let (inputs, values) = NONE.get_or_init(Default::default);
        Interpolator::new(inputs, values)
    }

    fn computer(world: &Node) -> &Node {
        &world.get("computers").unwrap().as_list().unwrap()[0]
    }

    const ONE: &str = "computers:\n  - id: a\n    copy:\n      - from: home/all\n";

    #[test]
    fn a_directory_becomes_home_relative_initial_files() {
        let files = Fake::with(&[
            ("home/all/notes.txt", b"hi\n"),
            ("home/all/Documents/plan.md", b"# plan\n"),
        ]);
        let world = seeded(ONE, &files);
        let seeded = computer(&world)
            .get("initial_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(
            seeded.keys().collect::<Vec<_>>(),
            ["Documents/plan.md", "notes.txt"]
        );
        assert_eq!(seeded.get("notes.txt").unwrap().as_str(), Some("hi\n"));
    }

    #[test]
    fn a_later_entry_overlays_an_earlier_one() {
        let files = Fake::with(&[
            ("home/all/notes.txt", b"shared\n"),
            ("home/all/keep.txt", b"keep\n"),
            ("home/ubuntu/notes.txt", b"ubuntu\n"),
        ]);
        let world = seeded(
            "computers:\n  - id: a\n    copy:\n      - from: home/all\n      - from: home/ubuntu\n",
            &files,
        );
        let seeded = computer(&world)
            .get("initial_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(seeded.get("notes.txt").unwrap().as_str(), Some("ubuntu\n"));
        assert_eq!(seeded.get("keep.txt").unwrap().as_str(), Some("keep\n"));
    }

    #[test]
    fn a_stated_file_beats_a_copied_one() {
        let files = Fake::with(&[("home/all/notes.txt", b"copied\n")]);
        let world = seeded(
            "computers:\n  - id: a\n    initial_files:\n      notes.txt: \"stated\\n\"\n    copy:\n      - from: home/all\n",
            &files,
        );
        let seeded = computer(&world)
            .get("initial_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(seeded.get("notes.txt").unwrap().as_str(), Some("stated\n"));
    }

    #[test]
    fn bytes_that_are_not_text_travel_base64() {
        let files = Fake::with(&[("home/all/logo.bin", &[0xff, 0x00, 0x41])]);
        let world = seeded(ONE, &files);
        let binary = computer(&world)
            .get("initial_binary_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(binary.get("logo.bin").unwrap().as_str(), Some("/wBB"));
        assert!(computer(&world).get("initial_files").is_none());
    }

    #[test]
    fn a_computer_with_no_binary_seeds_grows_no_empty_key() {
        let files = Fake::with(&[("home/all/notes.txt", b"hi\n")]);
        let world = seeded(ONE, &files);
        assert!(computer(&world).get("initial_binary_files").is_none());
    }

    #[test]
    fn a_destination_may_be_absolute_or_under_home() {
        assert_eq!(join("~", "a/b"), "a/b");
        assert_eq!(join("~/", "a/b"), "a/b");
        assert_eq!(join("~/Documents", "a"), "Documents/a");
        assert_eq!(join("/etc", "hosts"), "/etc/hosts");
        assert_eq!(join("/etc/", "hosts"), "/etc/hosts");
    }

    const PROFILES: &str = "profiles:\n  - id: mac\n    family: macos\n    home: /Users/{user}\n  - id: win\n    family: windows\n    home: C:/Users/{user}\n";

    #[test]
    fn a_root_directory_mirrors_the_machine_and_home_stays_relative() {
        let files = Fake::with(&[
            ("root/Users/alice/notes.txt", b"hi\n"),
            ("root/etc/motd", b"welcome\n"),
        ]);
        let world = seeded(
            &format!("{PROFILES}computers:\n  - id: a\n    profile: mac\n    user: alice\n    copy:\n      - {{from: root, to: /}}\n"),
            &files,
        );
        let seeded = computer(&world)
            .get("initial_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(
            seeded.keys().collect::<Vec<_>>(),
            ["/etc/motd", "notes.txt"]
        );
    }

    #[test]
    fn a_computer_file_is_seeded_from_the_root_beside_it() {
        let files = Fake::with(&[
            (
                "world.yml",
                b"schema_version: 1\nid: w\ninternet: false\nprofiles:\n  - {id: u, name: U, family: linux, home: \"/home/{user}\", shell: posix}\ninclude: [computers/*/computer.json]\n",
            ),
            (
                "computers/lab/computer.json",
                b"{\"id\": \"lab\", \"profile\": \"u\", \"address\": \"10.0.0.2\", \"user\": \"ada\", \"installed_apps\": [\"terminal\"]}",
            ),
            ("computers/lab/root/home/ada/notes.txt", b"hi\n"),
        ]);
        let resolved = crate::resolve("world.yml", &files, &BTreeMap::new()).unwrap();
        let lab = computer(&resolved.world).as_map().unwrap();
        assert_eq!(
            lab.keys().collect::<Vec<_>>(),
            [
                "id",
                "profile",
                "address",
                "user",
                "initial_files",
                "installed_apps"
            ]
        );
        let seeded = lab.get("initial_files").unwrap().as_map().unwrap();
        assert_eq!(seeded.keys().collect::<Vec<_>>(), ["notes.txt"]);
    }

    #[test]
    fn a_windows_root_starts_with_the_drive() {
        assert_eq!(mirror("C/Users/bob/a.txt", "C:/Users/bob", true), "a.txt");
        assert_eq!(
            mirror("D/data/a.txt", "C:/Users/bob", true),
            "D:/data/a.txt"
        );
        assert_eq!(
            mirror("C/Users/bob/a.txt", "/home/bob", false),
            "/C/Users/bob/a.txt"
        );
        assert_eq!(mirror("home/bobby/a", "/home/bob", false), "/home/bobby/a");
    }

    #[test]
    fn a_root_directory_may_name_the_user_by_input() {
        let files = Fake::with(&[("root/home/${WHO}/notes.txt", b"hi\n")]);
        let mut document = yaml::parse(
            "profiles:\n  - id: u\n    home: /home/{user}\ncomputers:\n  - id: a\n    profile: u\n    user: noor\n    copy:\n      - {from: root, to: /}\n",
        )
        .unwrap();
        let inputs = crate::inputs::parse(&yaml::parse("WHO: ada\n").unwrap(), "w.yml").unwrap();
        let values = BTreeMap::from([("WHO".to_string(), "noor".to_string())]);
        let mut interp = Interpolator::new(&inputs, &values);
        seed(
            &mut document,
            &files,
            None,
            "w.yml",
            &mut BTreeSet::new(),
            &mut interp,
        )
        .unwrap();
        let seeded = computer(&document)
            .get("initial_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(seeded.keys().collect::<Vec<_>>(), ["notes.txt"]);
    }

    #[test]
    fn tool_droppings_are_never_seeded() {
        let files = Fake::with(&[
            ("home/all/.DS_Store", b"junk"),
            ("home/all/project/.gitkeep", b""),
            ("home/all/project/main.rs", b"fn main() {}\n"),
        ]);
        let world = seeded(ONE, &files);
        let seeded = computer(&world)
            .get("initial_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(seeded.keys().collect::<Vec<_>>(), ["project/main.rs"]);
    }

    #[test]
    fn exclude_drops_what_it_matches() {
        let files = Fake::with(&[("home/all/a.txt", b"a"), ("home/all/b.tmp", b"b")]);
        let world = seeded(
            "computers:\n  - id: a\n    copy:\n      - from: home/all\n        exclude: [\"*.tmp\"]\n",
            &files,
        );
        let seeded = computer(&world)
            .get("initial_files")
            .unwrap()
            .as_map()
            .unwrap();
        assert_eq!(seeded.keys().collect::<Vec<_>>(), ["a.txt"]);
    }

    #[test]
    fn seeded_files_take_the_place_the_directive_held() {
        let files = Fake::with(&[("home/all/a.txt", b"a"), ("home/all/b.bin", &[0xff])]);
        let world = seeded(
            "computers:\n  - id: a\n    user: ada\n    copy:\n      - from: home/all\n    packages: [git]\n",
            &files,
        );
        assert_eq!(
            computer(&world)
                .as_map()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [
                "id",
                "user",
                "initial_files",
                "initial_binary_files",
                "packages"
            ]
        );
    }

    #[test]
    fn the_copy_block_does_not_survive_into_the_world() {
        let files = Fake::with(&[("home/all/a.txt", b"a")]);
        let world = seeded(ONE, &files);
        assert!(computer(&world).get("copy").is_none());
    }

    #[test]
    fn a_seed_over_the_ceiling_is_refused_and_names_the_computer() {
        let files = Fake::with(&[("home/all/big.txt", &[b'x'; 64])]);
        let mut document = yaml::parse(ONE).unwrap();
        let limits = yaml::parse("bytes_per_computer: 16\n").unwrap();
        let error = seed(
            &mut document,
            &files,
            Some(&limits),
            "w.yml",
            &mut BTreeSet::new(),
            &mut no_inputs(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("computer a would be seeded with 64 bytes"),
            "{error}"
        );
        assert!(error.contains("the limit is 16"), "{error}");
    }

    #[test]
    fn a_missing_directory_is_refused() {
        let files = Fake::with(&[("elsewhere/a.txt", b"a")]);
        let mut document = yaml::parse(ONE).unwrap();
        assert!(seed(
            &mut document,
            &files,
            None,
            "w.yml",
            &mut BTreeSet::new(),
            &mut no_inputs()
        )
        .is_err());
    }

    #[test]
    fn base64_matches_the_decoder_in_the_protocol_crate() {
        for case in [
            &b""[..],
            b"M",
            b"Ma",
            b"Man",
            b"hello world",
            &[0, 255, 128, 3],
        ] {
            let encoded = base64(case);
            assert_eq!(
                cw_protocol::decode_base64(&encoded).unwrap(),
                case,
                "{encoded}"
            );
        }
    }
}
