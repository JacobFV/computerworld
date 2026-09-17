//! Optional evaluation policy. Private task definitions are never part of actor observations.
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Predicate {
    Equals { pointer: String, value: Value },
    Exists { pointer: String },
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Not(Box<Predicate>),
}
impl Predicate {
    pub fn evaluate(&self, inspection: &Value) -> bool {
        match self {
            Self::Equals { pointer, value } => inspection.pointer(pointer) == Some(value),
            Self::Exists { pointer } => inspection.pointer(pointer).is_some(),
            Self::All(p) => p.iter().all(|v| v.evaluate(inspection)),
            Self::Any(p) => p.iter().any(|v| v.evaluate(inspection)),
            Self::Not(p) => !p.evaluate(inspection),
        }
    }
}
/// Created and retained by the evaluator. Never install task secrets in world metadata.
#[derive(Debug, Clone)]
pub struct Task {
    objective: String,
    predicate: Predicate,
    success_reward: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Evaluation {
    pub success: bool,
    pub reward: f64,
}
impl Task {
    pub fn new(objective: impl Into<String>, predicate: Predicate, success_reward: f64) -> Self {
        Self {
            objective: objective.into(),
            predicate,
            success_reward,
        }
    }
    pub fn objective(&self) -> &str {
        &self.objective
    }
    pub fn evaluate(&self, inspection: &Value) -> Evaluation {
        let success = self.predicate.evaluate(inspection);
        Evaluation {
            success,
            reward: if success { self.success_reward } else { 0.0 },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn predicates_are_read_only() {
        let state = serde_json::json!({"file":"ready","secret":"private"});
        let task = Task::new(
            "finish",
            Predicate::Equals {
                pointer: "/file".into(),
                value: "ready".into(),
            },
            1.0,
        );
        let before = state.clone();
        assert!(task.evaluate(&state).success);
        assert_eq!(state, before);
        assert!(!serde_json::to_string(&task.evaluate(&state))
            .unwrap()
            .contains("private"));
    }
}
