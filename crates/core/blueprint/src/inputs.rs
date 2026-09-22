//! Declared inputs, and the `${NAME}` interpolation that reads them.
//!
//! A blueprint may not reach into the ambient environment: every variable it
//! substitutes is declared up front, so `cw-world build` can say what a world
//! needs before it needs it, and so a resolved world records what it was built
//! from. An undeclared name is an error, and so is a declared name with neither
//! a value nor a default — a silently empty substitution is how a world starts
//! differing between two machines without anyone noticing.
use crate::node::{Map, Node};
use crate::Error;
use std::collections::{BTreeMap, BTreeSet};

/// One declared input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    pub description: String,
    pub default: Option<String>,
    /// When non-empty, the only values the input accepts.
    pub one_of: Vec<String>,
    /// A value that may never reach the resolved world: usable in a blueprint's
    /// source paths, refused anywhere it would be written into the artifact.
    pub secret: bool,
    /// Whether the world builds without it. An optional input may be left unset,
    /// and every `${NAME}` that reads it must then carry its own `:-` fallback —
    /// so a world still never substitutes a value nobody chose.
    pub optional: bool,
}

/// The `inputs:` block, in declaration order of the document (sorted, so two
/// runs report missing values in the same order).
pub type Inputs = BTreeMap<String, Input>;

/// Parse an `inputs:` mapping. A bare scalar is shorthand for a default.
pub fn parse(value: &Node, source: &str) -> Result<Inputs, Error> {
    let Some(map) = value.as_map() else {
        return Err(Error::at(
            source,
            "inputs must be a mapping of name to declaration",
        ));
    };
    let mut out = Inputs::new();
    for (name, decl) in map.iter() {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(Error::at(
                source,
                format!("input name {name:?} is not a NAME_LIKE_THIS identifier"),
            ));
        }
        let input = match decl {
            Node::Map(fields) => {
                crate::known_keys(
                    fields,
                    &["description", "default", "one_of", "secret", "optional"],
                    &format!("input {name}"),
                    source,
                )?;
                Input {
                    description: string_field(fields, "description")?.unwrap_or_default(),
                    default: string_field(fields, "default")?,
                    one_of: match fields.get("one_of") {
                        None | Some(Node::Null) => vec![],
                        Some(Node::List(items)) => items
                            .iter()
                            .map(Node::scalar_text)
                            .collect::<Option<Vec<_>>>()
                            .ok_or_else(|| {
                                Error::at(source, format!("input {name}: one_of takes scalars"))
                            })?,
                        Some(_) => {
                            return Err(Error::at(
                                source,
                                format!("input {name}: one_of is a list"),
                            ))
                        }
                    },
                    secret: matches!(fields.get("secret"), Some(Node::Bool(true))),
                    optional: matches!(fields.get("optional"), Some(Node::Bool(true))),
                }
            }
            Node::Null => Input::default(),
            other => Input {
                default: Some(other.scalar_text().ok_or_else(|| {
                    Error::at(source, format!("input {name}: default must be a scalar"))
                })?),
                ..Input::default()
            },
        };
        if let (Some(default), false) = (&input.default, input.one_of.is_empty()) {
            if !input.one_of.contains(default) {
                return Err(Error::at(
                    source,
                    format!("input {name}: default {default:?} is not one of its accepted values"),
                ));
            }
        }
        out.insert(name.to_string(), input);
    }
    Ok(out)
}

fn string_field(fields: &Map, key: &str) -> Result<Option<String>, Error> {
    match fields.get(key) {
        None | Some(Node::Null) => Ok(None),
        Some(other) => Ok(Some(other.scalar_text().ok_or_else(|| {
            Error::new(format!("{key} must be a scalar, not {}", other.kind()))
        })?)),
    }
}

/// Whether `text` is an input name, and so whether `${text}` is a substitution.
fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !text.starts_with(|c: char| c.is_ascii_digit())
}

/// The values the inputs resolve to, given what the environment supplied.
/// Reports every problem at once: being told about one missing variable per run
/// is how a five-variable world takes five runs to build.
pub fn resolve(
    inputs: &Inputs,
    supplied: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, Error> {
    let mut out = BTreeMap::new();
    let mut problems = Vec::new();
    for (name, input) in inputs {
        let value = match (supplied.get(name), &input.default) {
            (Some(v), _) => v.clone(),
            (None, Some(d)) => d.clone(),
            (None, None) if input.optional => continue,
            (None, None) => {
                let hint = if input.description.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", input.description)
                };
                problems.push(format!("  {name} is not set and has no default{hint}"));
                continue;
            }
        };
        if !input.one_of.is_empty() && !input.one_of.contains(&value) {
            problems.push(format!(
                "  {name} is {value:?}, which is not one of {}",
                input.one_of.join(", ")
            ));
            continue;
        }
        out.insert(name.clone(), value);
    }
    if problems.is_empty() {
        Ok(out)
    } else {
        Err(Error::new(format!(
            "the blueprint's inputs are not satisfied:\n{}",
            problems.join("\n")
        )))
    }
}

/// Substitute `${NAME}` and `${NAME:-fallback}` through every string in `value`,
/// keys included. `$${` is a literal `${`, so a world can carry the syntax as text.
///
/// Records which secret inputs were used, so the caller can refuse them where the
/// substitution would be written into the resolved artifact.
pub struct Interpolator<'a> {
    pub inputs: &'a Inputs,
    pub values: &'a BTreeMap<String, String>,
    pub used: BTreeSet<String>,
}

impl<'a> Interpolator<'a> {
    pub fn new(inputs: &'a Inputs, values: &'a BTreeMap<String, String>) -> Self {
        Self {
            inputs,
            values,
            used: BTreeSet::new(),
        }
    }

    /// Rewrite `value` in place. `where_` names the position for error messages.
    pub fn walk(&mut self, value: &mut Node, where_: &str) -> Result<(), Error> {
        match value {
            Node::String(s) => {
                if let Some(rewritten) = self.text(s, where_)? {
                    *s = rewritten;
                }
            }
            Node::List(items) => {
                for (i, item) in items.iter_mut().enumerate() {
                    self.walk(item, &format!("{where_}[{i}]"))?;
                }
            }
            Node::Map(map) => {
                let mut rebuilt = Map::new();
                for (key, mut item) in std::mem::take(map) {
                    let at = if where_.is_empty() {
                        key.clone()
                    } else {
                        format!("{where_}.{key}")
                    };
                    self.walk(&mut item, &at)?;
                    let key = self.text(&key, &at)?.unwrap_or(key);
                    if rebuilt.insert(key.clone(), item).is_some() {
                        return Err(Error::new(format!(
                            "{at}: two keys interpolate to the same name {key:?}"
                        )));
                    }
                }
                *map = rebuilt;
            }
            _ => {}
        }
        Ok(())
    }

    /// The substituted text, or `None` when there was nothing to substitute.
    pub fn text(&mut self, text: &str, where_: &str) -> Result<Option<String>, Error> {
        if !text.contains('$') {
            return Ok(None);
        }
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'$' {
                let start = i;
                while i < bytes.len() && bytes[i] != b'$' {
                    i += 1;
                }
                out.push_str(&text[start..i]);
                continue;
            }
            if text[i..].starts_with("$${") {
                out.push_str("${");
                i += 3;
                continue;
            }
            if !text[i..].starts_with("${") {
                out.push('$');
                i += 1;
                continue;
            }
            // Only a `${NAME}` or `${NAME:-fallback}` substitutes. Anything else
            // is left exactly as written, because seeded files are real files: a
            // workflow's `${{ matrix.os }}` and a shell script's `${1}` have to
            // survive a world being built around them.
            let close = text[i..].find('}').map(|at| at + i);
            let body = close.map(|close| &text[i + 2..close]);
            let parsed = body.and_then(|body| {
                let (name, fallback) = match body.split_once(":-") {
                    Some((name, fallback)) => (name, Some(fallback)),
                    None => (body, None),
                };
                is_name(name).then_some((name, fallback))
            });
            match (parsed, close) {
                (Some((name, fallback)), Some(close)) => {
                    out.push_str(&self.lookup(name, fallback, where_)?);
                    i = close + 1;
                }
                _ => {
                    out.push_str("${");
                    i += 2;
                }
            }
        }
        Ok(Some(out))
    }

    fn lookup(
        &mut self,
        name: &str,
        fallback: Option<&str>,
        where_: &str,
    ) -> Result<String, Error> {
        let Some(input) = self.inputs.get(name) else {
            let known = if self.inputs.is_empty() {
                "the blueprint declares none".to_string()
            } else {
                format!(
                    "declared: {}",
                    self.inputs.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            };
            return Err(Error::new(format!(
                "{where_}: ${{{name}}} is not a declared input ({known})"
            )));
        };
        if input.secret {
            self.used.insert(name.to_string());
        }
        match (self.values.get(name), fallback) {
            (Some(v), _) => Ok(v.clone()),
            (None, Some(f)) => Ok(f.to_string()),
            // `resolve` has already refused a required input that was not set, so
            // an absent value here means an optional one with nothing to fall back
            // on. Substituting an empty string is how a world quietly builds wrong.
            (None, None) => Err(Error::new(format!(
                "{where_}: ${{{name}}} is optional and was not set, so this use needs a \
                 fallback, as in ${{{name}:-something}}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml;

    fn inputs(text: &str) -> Inputs {
        parse(&yaml::parse(text).unwrap(), "test.yml").unwrap()
    }

    fn refused(text: &str) -> bool {
        parse(&yaml::parse(text).unwrap(), "test.yml").is_err()
    }

    fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn a_bare_scalar_declares_a_default() {
        let declared = inputs("DOMAIN: example.test\nPORT: 8080\n");
        assert_eq!(declared["DOMAIN"].default.as_deref(), Some("example.test"));
        assert_eq!(declared["PORT"].default.as_deref(), Some("8080"));
    }

    #[test]
    fn a_name_that_is_not_an_identifier_is_refused() {
        assert!(refused("my-input: x\n"));
    }

    #[test]
    fn an_unknown_declaration_key_is_refused() {
        assert!(refused("A:\n  defualt: x\n"));
    }

    #[test]
    fn a_default_outside_one_of_is_refused() {
        assert!(refused("R:\n  default: mars\n  one_of: [us, eu]\n"));
    }

    #[test]
    fn a_missing_value_without_a_default_is_refused_with_its_description() {
        let declared = inputs("TOKEN:\n  description: the API token\n");
        let error = resolve(&declared, &values(&[])).unwrap_err().to_string();
        assert!(error.contains("TOKEN is not set"), "{error}");
        assert!(error.contains("the API token"), "{error}");
    }

    #[test]
    fn every_unsatisfied_input_is_reported_at_once() {
        let declared = inputs("A: ~\nB: ~\nC: ok\n");
        let error = resolve(&declared, &values(&[])).unwrap_err().to_string();
        assert!(error.contains("  A is not set"), "{error}");
        assert!(error.contains("  B is not set"), "{error}");
        assert!(!error.contains("C is not set"), "{error}");
    }

    #[test]
    fn a_supplied_value_beats_the_default_and_must_be_accepted() {
        let declared = inputs("R:\n  default: us\n  one_of: [us, eu]\n");
        assert_eq!(
            resolve(&declared, &values(&[("R", "eu")])).unwrap()["R"],
            "eu"
        );
        let error = resolve(&declared, &values(&[("R", "mars")]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not one of us, eu"), "{error}");
    }

    #[test]
    fn substitution_reaches_strings_and_keys_at_every_depth() {
        let declared = inputs("HOST: wiki.internal\nUSER: ada\n");
        let resolved = resolve(&declared, &values(&[])).unwrap();
        let mut interp = Interpolator::new(&declared, &resolved);
        let mut doc = yaml::parse(
            "computers:\n  - user: ${USER}\n    files:\n      /home/${USER}/n: see ${HOST}\n",
        )
        .unwrap();
        interp.walk(&mut doc, "").unwrap();
        let expected = yaml::parse(
            "computers:\n  - user: ada\n    files:\n      /home/ada/n: see wiki.internal\n",
        )
        .unwrap();
        assert_eq!(doc, expected);
    }

    #[test]
    fn a_fallback_applies_only_when_nothing_was_supplied() {
        let declared = inputs("TAG:\n  optional: true\n");
        let resolved = resolve(&declared, &values(&[])).unwrap();
        let mut interp = Interpolator::new(&declared, &resolved);
        assert_eq!(
            interp.text("v${TAG:-latest}", "t").unwrap().unwrap(),
            "vlatest"
        );
        let supplied = values(&[("TAG", "2026")]);
        let mut interp = Interpolator::new(&declared, &supplied);
        assert_eq!(
            interp.text("v${TAG:-latest}", "t").unwrap().unwrap(),
            "v2026"
        );
    }

    #[test]
    fn an_undeclared_name_is_refused_and_says_what_is_declared() {
        let declared = inputs("HOST: h\n");
        let resolved = resolve(&declared, &values(&[])).unwrap();
        let mut interp = Interpolator::new(&declared, &resolved);
        let error = interp
            .text("${HSOT}", "computers[0].user")
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a declared input"), "{error}");
        assert!(error.contains("declared: HOST"), "{error}");
        assert!(error.contains("computers[0].user"), "{error}");
    }

    #[test]
    fn a_doubled_dollar_is_a_literal() {
        let declared = Inputs::new();
        let resolved = BTreeMap::new();
        let mut interp = Interpolator::new(&declared, &resolved);
        assert_eq!(
            interp.text("$${HOME}/x", "t").unwrap().unwrap(),
            "${HOME}/x"
        );
        assert_eq!(interp.text("cost: $5", "t").unwrap().unwrap(), "cost: $5");
    }

    #[test]
    fn what_is_not_an_input_name_is_left_exactly_as_written() {
        let declared = inputs("A: \"1\"\n");
        let resolved = resolve(&declared, &values(&[])).unwrap();
        let mut interp = Interpolator::new(&declared, &resolved);
        for text in [
            "runs-on: ${{ matrix.os }}",
            "echo ${1}",
            "${A-B}",
            "${A",
            "${}",
            "cd ${PWD:?no}",
        ] {
            assert_eq!(interp.text(text, "t").unwrap().unwrap(), text, "{text}");
        }
        assert_eq!(interp.text("${A}", "t").unwrap().unwrap(), "1");
    }

    #[test]
    fn keys_that_collide_after_substitution_are_refused() {
        let declared = inputs("A: same\n");
        let resolved = resolve(&declared, &values(&[])).unwrap();
        let mut interp = Interpolator::new(&declared, &resolved);
        let mut doc = yaml::parse("same: 1\n${A}: 2\n").unwrap();
        assert!(interp.walk(&mut doc, "").is_err());
    }

    #[test]
    fn using_a_secret_is_recorded() {
        let declared = inputs("T:\n  default: s3cret\n  secret: true\n");
        let resolved = resolve(&declared, &values(&[])).unwrap();
        let mut interp = Interpolator::new(&declared, &resolved);
        interp.text("${T}", "t").unwrap();
        assert!(interp.used.contains("T"));
    }

    #[test]
    fn an_optional_input_read_without_a_fallback_is_refused() {
        let declared = inputs("TAG:\n  optional: true\n");
        let resolved = resolve(&declared, &values(&[])).unwrap();
        let mut interp = Interpolator::new(&declared, &resolved);
        let error = interp.text("${TAG}", "t").unwrap_err().to_string();
        assert!(error.contains("needs a fallback"), "{error}");
    }

    #[test]
    fn text_without_a_dollar_is_left_alone() {
        let declared = Inputs::new();
        let resolved = BTreeMap::new();
        let mut interp = Interpolator::new(&declared, &resolved);
        assert_eq!(interp.text("plain", "t").unwrap(), None);
    }
}
