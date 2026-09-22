use crate::Vfs;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub files: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    pub executables: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageReceipt {
    pub version: String,
    pub files: Vec<String>,
    pub dependencies: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PackageManager {
    pub installed: BTreeMap<String, String>,
    pub catalog: BTreeMap<String, Vec<Package>>,
    pub receipts: BTreeMap<String, PackageReceipt>,
}
fn version(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split('.')
        .map(|n| n.split('-').next().unwrap_or("0").parse().unwrap_or(0))
        .collect()
}
pub fn version_matches(v: &str, range: &str) -> bool {
    let range = range.trim();
    if range.is_empty() || range == "*" || range == "latest" {
        return true;
    }
    if range.contains("||") {
        return range.split("||").any(|r| version_matches(v, r));
    }
    if range.split_whitespace().count() > 1 {
        return range.split_whitespace().all(|r| version_matches(v, r));
    }
    let a = version(v);
    if let Some(r) = range.strip_prefix('^') {
        let b = version(r);
        return a >= b && a.first() == b.first() && (b.first() != Some(&0) || a.get(1) == b.get(1));
    }
    if let Some(r) = range.strip_prefix('~') {
        let b = version(r);
        return a >= b && a.first() == b.first() && a.get(1) == b.get(1);
    }
    for (op, n) in [(">=", 2), ("<=", 2), (">", 1), ("<", 1), ("=", 1)] {
        if range.starts_with(op) {
            let b = version(&range[n..]);
            return match op {
                ">=" => a >= b,
                "<=" => a <= b,
                ">" => a > b,
                "<" => a < b,
                _ => a == b,
            };
        }
    }
    if range.contains('*') || range.contains('x') {
        return range
            .split('.')
            .zip(v.split('.'))
            .all(|(a, b)| a == "*" || a == "x" || a == b);
    }
    a == version(range)
}
impl PackageManager {
    pub fn register(&mut self, package: Package) {
        let versions = self.catalog.entry(package.name.clone()).or_default();
        versions.retain(|p| p.version != package.version);
        versions.push(package);
        versions.sort_by_key(|p| std::cmp::Reverse(version(&p.version)));
    }
    fn resolve(
        &self,
        requirements: &BTreeMap<String, Vec<String>>,
        selected: &BTreeMap<String, Package>,
    ) -> Result<BTreeMap<String, Package>, String> {
        let unsatisfied = requirements.iter().find(|(name, ranges)| {
            selected
                .get(*name)
                .map(|p| !ranges.iter().all(|r| version_matches(&p.version, r)))
                .unwrap_or(true)
        });
        let Some((name, ranges)) = unsatisfied else {
            return Ok(selected.clone());
        };
        if selected.contains_key(name) {
            return Err(format!("conflicting dependency: {name}"));
        }
        let candidates = self
            .catalog
            .get(name)
            .ok_or_else(|| format!("package not found: {name}"))?;
        for p in candidates
            .iter()
            .filter(|p| ranges.iter().all(|r| version_matches(&p.version, r)))
        {
            let mut next = selected.clone();
            next.insert(name.clone(), p.clone());
            let mut req = requirements.clone();
            for (dep, range) in &p.dependencies {
                req.entry(dep.clone()).or_default().push(range.clone());
            }
            if let Ok(solution) = self.resolve(&req, &next) {
                return Ok(solution);
            }
        }
        Err(format!(
            "no compatible version for {name}: {}",
            ranges.join(", ")
        ))
    }
    pub fn install(
        &mut self,
        spec: &str,
        vfs: &mut Vfs,
        user: &str,
        tick: u64,
    ) -> Result<Vec<String>, String> {
        let (name, range) = spec
            .rsplit_once('@')
            .filter(|(n, _)| !n.is_empty())
            .unwrap_or((spec, "*"));
        let mut req = BTreeMap::from([(name.to_string(), vec![range.to_string()])]);
        for (existing, v) in &self.installed {
            if existing != name {
                req.insert(existing.clone(), vec![format!("={v}")]);
            }
        }
        let solution = self.resolve(&req, &BTreeMap::new())?;
        let mut next = vfs.clone();
        let mut receipts = self.receipts.clone();
        let mut installed = self.installed.clone();
        let mut changed = Vec::new();
        for (name, p) in solution {
            if installed.get(&name) == Some(&p.version) {
                continue;
            }
            let mut files = p.files.clone();
            for exe in &p.executables {
                files
                    .entry(format!("/usr/bin/{exe}"))
                    .or_insert_with(|| format!("#!cw-package\n{name}\n{exe}\n").into_bytes());
            }
            for path in files.keys() {
                if let Some((owner, _)) = receipts
                    .iter()
                    .find(|(other, r)| *other != &name && r.files.contains(path))
                {
                    return Err(format!("file conflict: {path} owned by {owner}"));
                }
            }
            if let Some(old) = receipts.get(&name) {
                for path in &old.files {
                    if !files.contains_key(path) && next.exists(path) {
                        next.remove(path, false).map_err(|e| e.to_string())?;
                    }
                }
            }
            for (path, bytes) in &files {
                let parent = path.rsplit_once('/').map(|p| p.0).unwrap_or("/");
                next.mkdir_all(parent, user, tick)
                    .map_err(|e| e.to_string())?;
                next.write(path, bytes, user, tick)
                    .map_err(|e| e.to_string())?;
                if path.starts_with("/usr/bin/") {
                    next.chmod(path, 0o755).map_err(|e| e.to_string())?;
                }
            }
            receipts.insert(
                name.clone(),
                PackageReceipt {
                    version: p.version.clone(),
                    files: files.keys().cloned().collect(),
                    dependencies: p.dependencies.keys().cloned().collect(),
                },
            );
            installed.insert(name.clone(), p.version);
            changed.push(name);
        }
        *vfs = next;
        self.installed = installed;
        self.receipts = receipts;
        Ok(changed)
    }
    pub fn remove(&mut self, name: &str, vfs: &mut Vfs) -> Result<(), String> {
        let dependents: BTreeSet<_> = self
            .receipts
            .iter()
            .filter(|(_, r)| r.dependencies.iter().any(|d| d == name))
            .map(|(n, _)| n.clone())
            .collect();
        if !dependents.is_empty() {
            return Err(format!(
                "required by {}",
                dependents.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
        let receipt = self
            .receipts
            .get(name)
            .ok_or("package not installed")?
            .clone();
        let mut next = vfs.clone();
        for path in receipt.files {
            if next.exists(&path) {
                next.remove(&path, false).map_err(|e| e.to_string())?
            }
        }
        *vfs = next;
        self.receipts.remove(name);
        self.installed.remove(name);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn p(name: &str, v: &str, deps: &[(&str, &str)]) -> Package {
        Package {
            name: name.into(),
            version: v.into(),
            dependencies: deps
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            files: BTreeMap::new(),
            executables: vec![name.into()],
        }
    }
    #[test]
    fn dependency_transaction() {
        let mut pm = PackageManager::default();
        pm.register(p("lib", "1.0.0", &[]));
        pm.register(p("lib", "2.0.0", &[]));
        pm.register(p("app", "1.0.0", &[("lib", "^1.0.0")]));
        let mut fs = Vfs::new(true);
        assert_eq!(pm.install("app", &mut fs, "u", 0).unwrap(), ["app", "lib"]);
        assert_eq!(pm.installed["lib"], "1.0.0");
        assert!(fs.exists("/usr/bin/app"));
        let before = fs.clone();
        pm.register(p("bad", "1.0.0", &[("missing", "*")]));
        assert!(pm.install("bad", &mut fs, "u", 0).is_err());
        assert_eq!(before, fs);
        assert!(pm.remove("lib", &mut fs).is_err());
        pm.remove("app", &mut fs).unwrap();
        pm.remove("lib", &mut fs).unwrap();
    }
    #[test]
    fn ranges() {
        assert!(version_matches("1.9.0", "^1.2.0"));
        assert!(!version_matches("2.0.0", "^1.2.0"));
        assert!(version_matches("1.5.0", ">=1.0.0 <2.0.0"));
        assert!(!version_matches("1.6.0", "~1.5.0"));
    }
}
