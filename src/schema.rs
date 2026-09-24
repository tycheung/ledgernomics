//! Project and slice schema with budget validation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

pub const DEFAULT_MAX_FILES: u32 = 3;
pub const DEFAULT_MAX_LOC: u32 = 120;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SliceStatus {
    #[default]
    Open,
    Blocked,
    Done,
}

impl FromStr for SliceStatus {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "blocked" => Ok(Self::Blocked),
            "done" => Ok(Self::Done),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Project {
    pub goals: String,
    pub non_goals: String,
    pub status: String,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            goals: String::new(),
            non_goals: String::new(),
            status: "active".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Slice {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub target_paths: Vec<String>,
    pub acceptance: String,
    pub out_of_scope: String,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default = "default_max_files")]
    pub max_files: u32,
    #[serde(default = "default_max_loc")]
    pub max_loc: u32,
    #[serde(default)]
    pub status: SliceStatus,
    #[serde(default)]
    pub blockers: String,
}

fn default_max_files() -> u32 {
    DEFAULT_MAX_FILES
}
fn default_max_loc() -> u32 {
    DEFAULT_MAX_LOC
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("slice id is required")]
    MissingId,
    #[error("acceptance is required")]
    MissingAcceptance,
    #[error("target_paths must be non-empty")]
    MissingTargetPaths,
    #[error("out_of_scope is required")]
    MissingOutOfScope,
}

impl Slice {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.id.trim().is_empty() {
            return Err(ValidationError::MissingId);
        }
        if self.acceptance.trim().is_empty() {
            return Err(ValidationError::MissingAcceptance);
        }
        if self.target_paths.is_empty() || self.target_paths.iter().all(|p| p.trim().is_empty()) {
            return Err(ValidationError::MissingTargetPaths);
        }
        if self.out_of_scope.trim().is_empty() {
            return Err(ValidationError::MissingOutOfScope);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionShape {
    Feature,
    Steward,
}

impl FromStr for SessionShape {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.trim().to_ascii_lowercase().as_str() {
            "feature" => Ok(Self::Feature),
            "steward" => Ok(Self::Steward),
            _ => Err(()),
        }
    }
}

impl SessionShape {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Feature => "feature",
            Self::Steward => "steward",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Feature => {
                "Feature micro-slice: implement one slice with TARGET_PATHS, ACCEPTANCE, OUT OF SCOPE; stop at DoD."
            }
            Self::Steward => {
                "Steward cleanup priority (do not reorder):\n\
1. Dual SSOT / domain correctness\n\
2. Security / GET writes / fail-closed\n\
3. Contract sync (delete dead clients same day)\n\
4. Tests locking the fix\n\
5. Encode the lesson in a rule/ADR\n\
6. Oversized-file splits only if ceilings fail\n\
Out of scope: aesthetic rewrites, unrelated renames, new features."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_slice() -> Slice {
        Slice {
            id: "led001".into(),
            title: "t".into(),
            goal: "g".into(),
            target_paths: vec!["src/a.rs".into()],
            acceptance: "tests green".into(),
            out_of_scope: "UI".into(),
            tests: vec![],
            max_files: 3,
            max_loc: 120,
            status: SliceStatus::Open,
            blockers: String::new(),
        }
    }

    #[test]
    fn validate_ok() {
        assert!(valid_slice().validate().is_ok());
    }

    #[test]
    fn reject_missing_out_of_scope() {
        let mut s = valid_slice();
        s.out_of_scope = "  ".into();
        assert_eq!(s.validate(), Err(ValidationError::MissingOutOfScope));
    }

    #[test]
    fn reject_empty_acceptance() {
        let mut s = valid_slice();
        s.acceptance.clear();
        assert_eq!(s.validate(), Err(ValidationError::MissingAcceptance));
    }

    #[test]
    fn reject_empty_paths() {
        let mut s = valid_slice();
        s.target_paths.clear();
        assert_eq!(s.validate(), Err(ValidationError::MissingTargetPaths));
    }
}
