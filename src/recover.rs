//! Cache-aware context assembly — stable vs volatile tiers + fingerprint.

use crate::schema::{Project, SessionShape, Slice, SliceStatus};
use crate::store::Attempt;
use crate::store::LedgerStats;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub const HOST_HINT: &str = "Pin `stable` until `fingerprint` changes; append `volatile` only this turn (or omit). Do not re-paste stable mid-slice.";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StableSlice {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub acceptance: String,
    pub out_of_scope: String,
    pub target_paths: Vec<String>,
    pub max_files: u32,
    pub max_loc: u32,
    pub status: SliceStatus,
    pub blockers: String,
}

impl From<&Slice> for StableSlice {
    fn from(s: &Slice) -> Self {
        Self {
            id: s.id.clone(),
            title: s.title.clone(),
            goal: s.goal.clone(),
            acceptance: s.acceptance.clone(),
            out_of_scope: s.out_of_scope.clone(),
            target_paths: s.target_paths.clone(),
            max_files: s.max_files,
            max_loc: s.max_loc,
            status: s.status.clone(),
            blockers: s.blockers.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StableContext {
    pub project_status: String,
    pub goals: String,
    pub non_goals: String,
    pub current_slice: Option<StableSlice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steward_priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct VolatileContext {
    pub progress_excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_attempt: Option<AttemptHead>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<LedgerStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttemptHead {
    pub id: String,
    pub slice_id: String,
    pub summary: String,
    pub failure_mode: String,
}

impl From<&Attempt> for AttemptHead {
    fn from(a: &Attempt) -> Self {
        Self {
            id: a.id.clone(),
            slice_id: a.slice_id.clone(),
            summary: a.summary.clone(),
            failure_mode: a.failure_mode.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AssemblePacket {
    pub ok: bool,
    pub budget_chars: usize,
    pub used_chars: usize,
    pub status: String,
    pub shape: String,
    pub fingerprint: String,
    pub host_hint: String,
    pub stable: StableContext,
    pub volatile: VolatileContext,
    pub omissions: Vec<String>,
    pub message: String,
}

pub type RecoverPacket = AssemblePacket;

pub fn fingerprint_stable(stable: &StableContext) -> String {
    let body = serde_json::to_string(stable).unwrap_or_default();
    let mut h = DefaultHasher::new();
    body.hash(&mut h);
    format!("fp-{:016x}", h.finish())
}

pub fn build_stable(
    project: &Project,
    current: Option<&Slice>,
    shape: SessionShape,
) -> StableContext {
    StableContext {
        project_status: project.status.clone(),
        goals: project.goals.clone(),
        non_goals: project.non_goals.clone(),
        current_slice: current.map(StableSlice::from),
        steward_priority: match shape {
            SessionShape::Steward => Some(shape.description().to_string()),
            SessionShape::Feature => None,
        },
    }
}

pub struct VolatileInputs<'a> {
    pub progress: &'a str,
    pub latest_attempt: Option<&'a Attempt>,
    pub stats: Option<&'a LedgerStats>,
}

pub fn assemble_context(
    project: &Project,
    current: Option<&Slice>,
    shape: SessionShape,
    budget_chars: usize,
    volatile_in: VolatileInputs<'_>,
) -> AssemblePacket {
    let shape_s = shape.as_str();
    let stable = build_stable(project, current, shape);
    let fingerprint = fingerprint_stable(&stable);

    let progress_excerpt = crate::util::truncate(volatile_in.progress, 400);

    let mut pkt = AssemblePacket {
        ok: true,
        budget_chars,
        used_chars: 0,
        status: "ok".into(),
        shape: shape_s.into(),
        fingerprint,
        host_hint: HOST_HINT.into(),
        stable,
        volatile: VolatileContext::default(),
        omissions: Vec::new(),
        message: "assembled cache-aware context".into(),
    };

    let core_chars = json_chars(&pkt);
    if core_chars > budget_chars {
        pkt.ok = false;
        pkt.used_chars = core_chars;
        pkt.status = "BUDGET_EXCEEDED".into();
        pkt.omissions
            .push(format!("critical_packet_chars={core_chars}"));
        pkt.message = format!(
            "BUDGET_EXCEEDED: critical context is {core_chars} chars > budget {budget_chars}"
        );
        return pkt;
    }

    pkt.volatile.progress_excerpt = progress_excerpt;
    if json_chars(&pkt) > budget_chars {
        pkt.volatile.progress_excerpt.clear();
        pkt.omissions.push("progress_excerpt".into());
    }

    if let Some(a) = volatile_in.latest_attempt {
        pkt.volatile.latest_attempt = Some(AttemptHead::from(a));
        if json_chars(&pkt) > budget_chars {
            pkt.volatile.latest_attempt = None;
            pkt.omissions.push("latest_attempt".into());
        }
    }
    if let Some(st) = volatile_in.stats {
        pkt.volatile.stats = Some(st.clone());
        if json_chars(&pkt) > budget_chars {
            pkt.volatile.stats = None;
            pkt.omissions.push("stats".into());
        }
    }

    pkt.used_chars = json_chars(&pkt);
    pkt
}

pub fn recover_context(
    project: &Project,
    current: Option<&Slice>,
    progress: &str,
    shape: SessionShape,
    budget_chars: usize,
) -> AssemblePacket {
    assemble_context(
        project,
        current,
        shape,
        budget_chars,
        VolatileInputs {
            progress,
            latest_attempt: None,
            stats: None,
        },
    )
}

fn json_chars<T: Serialize>(v: &T) -> usize {
    serde_json::to_string(v)
        .map(|s| s.chars().count())
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice() -> Slice {
        Slice {
            id: "led001".into(),
            title: "t".into(),
            goal: "g".into(),
            target_paths: vec!["a.rs".into()],
            acceptance: "ok".into(),
            out_of_scope: "ui".into(),
            tests: vec![],
            max_files: 3,
            max_loc: 120,
            status: SliceStatus::Open,
            blockers: "none".into(),
        }
    }

    fn project() -> Project {
        Project {
            goals: "x".into(),
            non_goals: "y".into(),
            status: "active".into(),
        }
    }

    #[test]
    fn recover_includes_out_of_scope() {
        let p = project();
        let s = slice();
        let pkt = recover_context(&p, Some(&s), "progress", SessionShape::Feature, 8000);
        assert!(pkt.ok);
        assert_eq!(
            pkt.stable.current_slice.as_ref().unwrap().out_of_scope,
            "ui"
        );
        assert_eq!(pkt.stable.goals, "x");
        assert!(pkt.fingerprint.starts_with("fp-"));
        assert!(!pkt.host_hint.is_empty());
    }

    #[test]
    fn fingerprint_ignores_volatile_progress() {
        let p = project();
        let s = slice();
        let a = recover_context(&p, Some(&s), "progress-a", SessionShape::Feature, 8000);
        let b = recover_context(
            &p,
            Some(&s),
            "progress-b-changed",
            SessionShape::Feature,
            8000,
        );
        assert_eq!(a.fingerprint, b.fingerprint);
        assert_ne!(a.volatile.progress_excerpt, b.volatile.progress_excerpt);
    }

    #[test]
    fn fingerprint_changes_when_goals_change() {
        let mut p = project();
        let s = slice();
        let a = recover_context(&p, Some(&s), "p", SessionShape::Feature, 8000);
        p.goals = "changed".into();
        let b = recover_context(&p, Some(&s), "p", SessionShape::Feature, 8000);
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn tiny_budget_exceeded_still_returns_critical() {
        let p = Project::default();
        let s = slice();
        let pkt = recover_context(&p, Some(&s), "progress text", SessionShape::Feature, 10);
        assert!(!pkt.ok);
        assert_eq!(pkt.status, "BUDGET_EXCEEDED");
        assert_eq!(
            pkt.stable.current_slice.as_ref().unwrap().out_of_scope,
            "ui"
        );
    }
}
