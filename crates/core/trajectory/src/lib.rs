//! Portable append-only hash chained journals. Verification detects edits and reordering.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry<T> {
    pub index: u64,
    pub previous_hash: String,
    pub hash: String,
    pub value: T,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Journal<T> {
    pub entries: Vec<Entry<T>>,
}
impl<T> Default for Journal<T> {
    fn default() -> Self {
        Self { entries: vec![] }
    }
}
pub fn stable_hash<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}
impl<T: Serialize> Journal<T> {
    pub fn append(&mut self, value: T) -> Result<&Entry<T>, serde_json::Error> {
        let index = self.entries.len() as u64;
        let previous_hash = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_default();
        let hash = stable_hash(&(index, &previous_hash, &value))?;
        self.entries.push(Entry {
            index,
            previous_hash,
            hash,
            value,
        });
        Ok(self.entries.last().unwrap())
    }
    pub fn verify(&self) -> Result<bool, serde_json::Error> {
        let mut previous = "";
        for (index, e) in self.entries.iter().enumerate() {
            if e.index != index as u64
                || e.previous_hash != previous
                || e.hash != stable_hash(&(e.index, &e.previous_hash, &e.value))?
            {
                return Ok(false);
            }
            previous = &e.hash;
        }
        Ok(true)
    }
    pub fn jsonl(&self) -> Result<String, serde_json::Error> {
        let mut output = String::new();
        for entry in &self.entries {
            output.push_str(&serde_json::to_string(entry)?);
            output.push('\n');
        }
        Ok(output)
    }
}
impl<T: Serialize + for<'de> Deserialize<'de>> Journal<T> {
    pub fn from_jsonl(input: &str) -> Result<Self, String> {
        let journal = Self {
            entries: input
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(serde_json::from_str)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?,
        };
        if !journal.verify().map_err(|e| e.to_string())? {
            return Err("invalid journal hash chain".into());
        }
        Ok(journal)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_tampering() {
        let mut j = Journal::default();
        j.append("first").unwrap();
        j.append("second").unwrap();
        assert!(j.verify().unwrap());
        let text = j.jsonl().unwrap();
        assert_eq!(
            Journal::<String>::from_jsonl(&text).unwrap().entries.len(),
            2
        );
        j.entries[0].value = "other";
        assert!(!j.verify().unwrap());
    }
}
