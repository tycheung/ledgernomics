//! Single-prompt epic scoping: draft + clarifications + optional commit.

use crate::schema::{Project, Slice, SliceStatus, DEFAULT_MAX_FILES, DEFAULT_MAX_LOC};
use serde::{Deserialize, Serialize};

pub const SCOPE_HOST_HINT: &str = "On a single epic prompt: call scope_epic with the brief \
(and any goals/non_goals/slices you drafted). If clarifications is non-empty, ask the user \
those questions—do not commit. When ok, call again with commit=true. Then each slice turn: \
prefix_fingerprint / assemble_context; pin stable until fingerprint changes.";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScopeSliceDraft {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub target_paths: Vec<String>,
    #[serde(default)]
    pub acceptance: String,
    #[serde(default)]
    pub out_of_scope: String,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default = "default_files")]
    pub max_files: u32,
    #[serde(default = "default_loc")]
    pub max_loc: u32,
}

fn default_files() -> u32 {
    DEFAULT_MAX_FILES
}
fn default_loc() -> u32 {
    DEFAULT_MAX_LOC
}

impl From<ScopeSliceDraft> for Slice {
    fn from(d: ScopeSliceDraft) -> Self {
        let title = if d.title.trim().is_empty() {
            d.id.clone()
        } else {
            d.title
        };
        let goal = if d.goal.trim().is_empty() {
            title.clone()
        } else {
            d.goal
        };
        Self {
            id: d.id,
            title,
            goal,
            target_paths: d.target_paths,
            acceptance: d.acceptance,
            out_of_scope: d.out_of_scope,
            tests: d.tests,
            max_files: d.max_files,
            max_loc: d.max_loc,
            status: SliceStatus::Open,
            blockers: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Clarification {
    pub field: String,
    pub question: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeResult {
    pub ok: bool,
    pub status: String,
    pub brief: String,
    pub goals: String,
    pub non_goals: String,
    pub slices: Vec<Slice>,
    pub clarifications: Vec<Clarification>,
    pub committed: bool,
    pub host_hint: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ScopeInput {
    pub brief: String,
    pub goals: String,
    pub non_goals: String,
    pub slices: Vec<ScopeSliceDraft>,
    pub commit: bool,
}

pub fn scope_epic(input: ScopeInput) -> ScopeResult {
    let brief = input.brief.trim().to_string();
    let mut goals = input.goals.trim().to_string();
    let mut non_goals = input.non_goals.trim().to_string();
    let mut drafts = input.slices;
    let mut clarifications = Vec::new();

    if brief.is_empty() {
        clarifications.push(Clarification {
            field: "brief".into(),
            question: "What should we build? Paste one epic prompt / outcome.".into(),
        });
    }

    if goals.is_empty() || non_goals.is_empty() || drafts.is_empty() {
        let parsed = parse_brief(&brief);
        if goals.is_empty() {
            goals = parsed.goals;
        }
        if non_goals.is_empty() {
            non_goals = parsed.non_goals;
        }
        if drafts.is_empty() {
            drafts = parsed.slices;
        }
        clarifications.extend(parsed.clarifications);
    }

    if goals.trim().is_empty() {
        clarifications.push(Clarification {
            field: "goals".into(),
            question: "What are the concrete success goals for this epic?".into(),
        });
    }
    if non_goals.trim().is_empty() {
        clarifications.push(Clarification {
            field: "non_goals".into(),
            question: "What is explicitly out of scope / non-goals?".into(),
        });
    }
    if drafts.is_empty() {
        clarifications.push(Clarification {
            field: "slices".into(),
            question:
                "List micro-slices (id, acceptance, target_paths, out_of_scope) to execute in order."
                    .into(),
        });
    }

    let mut slices = Vec::new();
    for (i, d) in drafts.into_iter().enumerate() {
        let slice = Slice::from(d);
        if let Err(e) = slice.validate() {
            clarifications.push(Clarification {
                field: format!("slices[{i}].{}", slice.id),
                question: format!(
                    "Slice `{}` invalid: {e}. Fix acceptance, paths, out_of_scope.",
                    slice.id
                ),
            });
        }
        slices.push(slice);
    }

    let mut seen = std::collections::HashSet::new();
    clarifications.retain(|c| seen.insert(c.field.clone()));

    let ready = clarifications.is_empty();
    let (status, message) = if !ready {
        (
            "needs_clarification",
            "Ask the user the clarifications, then call scope_epic again (commit=true when ready)."
                .into(),
        )
    } else if input.commit {
        (
            "ready_to_commit",
            "Scope valid; caller should persist via store commit.".into(),
        )
    } else {
        (
            "preview",
            "Scope looks valid. Re-call with commit=true to write the ledger.".into(),
        )
    };

    ScopeResult {
        ok: ready,
        status: status.into(),
        brief,
        goals,
        non_goals,
        slices,
        clarifications,
        committed: false,
        host_hint: SCOPE_HOST_HINT.into(),
        message,
    }
}

struct ParsedBrief {
    goals: String,
    non_goals: String,
    slices: Vec<ScopeSliceDraft>,
    clarifications: Vec<Clarification>,
}

fn parse_brief(brief: &str) -> ParsedBrief {
    let mut goals = String::new();
    let mut non_goals = String::new();
    let mut slices = Vec::new();
    let mut clarifications = Vec::new();

    let lower = brief.to_ascii_lowercase();
    if let Some(g) = section_after(&lower, brief, &["goals:", "goal:"]) {
        goals = g;
    }
    if let Some(n) = section_after(
        &lower,
        brief,
        &[
            "non-goals:",
            "non_goals:",
            "nongoals:",
            "out of scope:",
            "non goals:",
        ],
    ) {
        non_goals = n;
    }

    for (idx, line) in brief.lines().enumerate() {
        let t = line.trim();
        let title = t
            .strip_prefix('-')
            .or_else(|| t.strip_prefix('*'))
            .map(|s| s.trim())
            .or_else(|| {
                let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 && t[digits..].starts_with('.') {
                    Some(t[digits + 1..].trim())
                } else {
                    None
                }
            });
        if let Some(title) = title {
            if title.is_empty() || title.len() > 200 {
                continue;
            }
            let id = format!("scp{:03}", idx + 1);
            slices.push(ScopeSliceDraft {
                id,
                title: title.into(),
                goal: title.into(),
                target_paths: vec![],
                acceptance: String::new(),
                out_of_scope: String::new(),
                tests: vec![],
                max_files: DEFAULT_MAX_FILES,
                max_loc: DEFAULT_MAX_LOC,
            });
        }
    }

    if goals.is_empty() {
        let para = brief
            .split("\n\n")
            .map(|p| p.trim())
            .find(|p| !p.is_empty())
            .unwrap_or(brief.trim());
        goals = crate::util::truncate(para, 280);
        if !goals.is_empty() {
            clarifications.push(Clarification {
                field: "goals".into(),
                question: format!("Confirm or rewrite goals (drafted from brief): {goals}"),
            });
        }
    }

    if non_goals.is_empty() {
        clarifications.push(Clarification {
            field: "non_goals".into(),
            question: "What should we explicitly not do (non-goals / out of scope for the epic)?"
                .into(),
        });
    }

    if !slices.is_empty() {
        clarifications.push(Clarification {
            field: "slices".into(),
            question: "Provisional slices were inferred from list lines. Confirm ids and fill acceptance, target_paths, and out_of_scope for each before commit.".into(),
        });
    }

    ParsedBrief {
        goals,
        non_goals,
        slices,
        clarifications,
    }
}

fn section_after(lower: &str, original: &str, markers: &[&str]) -> Option<String> {
    for m in markers {
        if let Some(pos) = lower.find(m) {
            let start = pos + m.len();
            let rest = &original[start..];
            let end = rest
                .find('\n')
                .map(|i| {
                    let after = &rest[i..];
                    let mut e = i;
                    for (j, line) in after.lines().enumerate() {
                        if j == 0 {
                            continue;
                        }
                        let l = line.trim().to_ascii_lowercase();
                        if l.is_empty()
                            || l.starts_with("goals:")
                            || l.starts_with("non-")
                            || l.starts_with("out of scope")
                            || l.starts_with("slices:")
                        {
                            break;
                        }
                        e += line.len() + 1;
                    }
                    e
                })
                .unwrap_or(rest.len());
            let text = rest[..end].trim().to_string();
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

pub fn project_from_scope(r: &ScopeResult) -> Project {
    Project {
        goals: r.goals.clone(),
        non_goals: r.non_goals.clone(),
        status: "active".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_only_needs_clarification() {
        let r = scope_epic(ScopeInput {
            brief: "Build a cache-aware ledger for agents.".into(),
            goals: String::new(),
            non_goals: String::new(),
            slices: vec![],
            commit: false,
        });
        assert!(!r.ok);
        assert_eq!(r.status, "needs_clarification");
        assert!(!r.goals.is_empty());
        assert!(r.clarifications.iter().any(|c| c.field == "non_goals"));
        assert!(!r.committed);
    }

    #[test]
    fn full_draft_preview_ok() {
        let r = scope_epic(ScopeInput {
            brief: "Ship scope_epic".into(),
            goals: "scope from one prompt".into(),
            non_goals: "LLM inside MCP".into(),
            slices: vec![ScopeSliceDraft {
                id: "s1".into(),
                title: "tool".into(),
                goal: "wire tool".into(),
                target_paths: vec!["src/scope.rs".into()],
                acceptance: "tests green".into(),
                out_of_scope: "SDK".into(),
                tests: vec![],
                max_files: 3,
                max_loc: 120,
            }],
            commit: false,
        });
        assert!(r.ok);
        assert_eq!(r.status, "preview");
        assert_eq!(r.slices.len(), 1);
    }

    #[test]
    fn invalid_slice_clarifies() {
        let r = scope_epic(ScopeInput {
            brief: "x".into(),
            goals: "g".into(),
            non_goals: "n".into(),
            slices: vec![ScopeSliceDraft {
                id: "s1".into(),
                title: "t".into(),
                goal: "g".into(),
                target_paths: vec!["a.rs".into()],
                acceptance: "ok".into(),
                out_of_scope: String::new(),
                tests: vec![],
                max_files: 3,
                max_loc: 120,
            }],
            commit: true,
        });
        assert!(!r.ok);
        assert!(r.clarifications.iter().any(|c| c.field.contains("s1")));
    }
}
