//! Host-independent logical time, SplitMix64 streams and ordered continuations.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const RNG_ALGORITHM: &str = "sha256-named-splitmix64-v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clock {
    tick: u64,
}
impl Clock {
    pub fn new(tick: u64) -> Self {
        Self { tick }
    }
    pub fn now(&self) -> u64 {
        self.tick
    }
    pub fn advance(&mut self, delta: u64) -> Result<u64, String> {
        self.tick = self
            .tick
            .checked_add(delta)
            .ok_or("logical clock overflow")?;
        Ok(self.tick)
    }
    pub fn advance_to(&mut self, tick: u64) -> Result<(), String> {
        if tick < self.tick {
            return Err("cannot move logical clock backwards".into());
        }
        self.tick = tick;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Determinism {
    seed: u64,
    streams: BTreeMap<String, u64>,
    ids: BTreeMap<String, u64>,
}
impl Determinism {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            streams: BTreeMap::new(),
            ids: BTreeMap::new(),
        }
    }
    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn next_u64(&mut self, stream: &str) -> u64 {
        let state = self.streams.entry(stream.into()).or_insert_with(|| {
            let mut hash = Sha256::new();
            hash.update(RNG_ALGORITHM);
            hash.update(self.seed.to_le_bytes());
            hash.update((stream.len() as u64).to_le_bytes());
            hash.update(stream.as_bytes());
            u64::from_le_bytes(hash.finalize()[..8].try_into().unwrap())
        });
        *state = state.wrapping_add(0x9e3779b97f4a7c15);
        mix(*state)
    }
    /// Unbiased sample in [0, upper); rejection ordering is part of v1 semantics.
    pub fn range(&mut self, stream: &str, upper: u64) -> Result<u64, String> {
        if upper == 0 {
            return Err("empty random range".into());
        }
        let threshold = upper.wrapping_neg() % upper;
        loop {
            let n = self.next_u64(stream);
            if n >= threshold {
                return Ok(n % upper);
            }
        }
    }
    pub fn next_id(&mut self, namespace: &str) -> Result<String, String> {
        let id = self.ids.entry(namespace.into()).or_default();
        let next = id.checked_add(1).ok_or("id counter overflow")?;
        let result = format!("{namespace}:{id}");
        *id = next;
        Ok(result)
    }
}
pub fn mix(mut n: u64) -> u64 {
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^ (n >> 31)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scheduled<T> {
    pub due: u64,
    pub phase: u16,
    pub sequence: u64,
    pub value: T,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scheduler<T> {
    next_sequence: u64,
    queue: BTreeMap<u64, Vec<Scheduled<T>>>,
}
impl<T> Default for Scheduler<T> {
    fn default() -> Self {
        Self {
            next_sequence: 0,
            queue: BTreeMap::new(),
        }
    }
}
impl<T> Scheduler<T> {
    pub fn schedule(&mut self, now: u64, due: u64, phase: u16, value: T) -> Result<u64, String> {
        if due < now {
            return Err("cannot schedule into the past".into());
        }
        let sequence = self.next_sequence;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or("scheduler sequence overflow")?;
        let bucket = self.queue.entry(due).or_default();
        let item = Scheduled {
            due,
            phase,
            sequence,
            value,
        };
        let index = bucket.partition_point(|e| (e.phase, e.sequence) <= (phase, sequence));
        bucket.insert(index, item);
        Ok(sequence)
    }
    pub fn validate(&self, now: u64) -> Result<(), String> {
        let mut sequences = std::collections::BTreeSet::new();
        for (due, bucket) in &self.queue {
            if *due < now || bucket.is_empty() {
                return Err("invalid scheduler deadline or empty bucket".into());
            }
            let mut previous = None;
            for event in bucket {
                let key = (event.phase, event.sequence);
                if event.due != *due
                    || event.sequence >= self.next_sequence
                    || !sequences.insert(event.sequence)
                    || previous.is_some_and(|p| p >= key)
                {
                    return Err("invalid scheduler order or sequence".into());
                }
                previous = Some(key);
            }
        }
        Ok(())
    }
    pub fn next_due(&self) -> Option<u64> {
        self.queue.first_key_value().map(|(k, _)| *k)
    }
    pub fn len(&self) -> usize {
        self.queue.values().map(Vec::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub fn pop_due(&mut self, through: u64) -> Option<Scheduled<T>> {
        let due = self.next_due()?;
        if due > through {
            return None;
        }
        let bucket = self.queue.get_mut(&due).unwrap();
        let item = bucket.remove(0);
        if bucket.is_empty() {
            self.queue.remove(&due);
        }
        Some(item)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Scheduled<T>> {
        self.queue.values().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splitmix_golden() {
        assert_eq!(mix(0x9e3779b97f4a7c15), 0xe220a8397b1dcdaf);
        assert_eq!(mix(0x3c6ef372fe94f82a), 0x6e789e6aa1b965f4);
    }
    #[test]
    fn streams_are_independent_and_portable() {
        let mut a = Determinism::new(7);
        let mut b = a.clone();
        a.next_u64("unrelated");
        assert_eq!(a.next_u64("mail/init"), b.next_u64("mail/init"));
        let mut c: Determinism = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(a.next_u64("mail/init"), c.next_u64("mail/init"));
        assert_ne!(
            a.next_u64("mail/init"),
            Determinism::new(8).next_u64("mail/init")
        );
    }
    #[test]
    fn scheduler_order_and_checkpoint() {
        let mut s = Scheduler::default();
        s.schedule(0, 5, 2, "third").unwrap();
        s.schedule(0, 5, 1, "first").unwrap();
        s.schedule(0, 5, 1, "second").unwrap();
        assert!(s.pop_due(4).is_none());
        let json = serde_json::to_string(&s).unwrap();
        let mut restored: Scheduler<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.pop_due(5).unwrap().value, "first");
        assert_eq!(restored.pop_due(5).unwrap().value, "second");
        assert_eq!(restored.pop_due(5).unwrap().value, "third");
        assert!(restored.is_empty());
    }
    #[test]
    fn checked_clock() {
        let mut c = Clock::new(u64::MAX);
        assert!(c.advance(1).is_err());
        assert_eq!(c.now(), u64::MAX);
        assert!(c.advance_to(0).is_err());
    }
}
