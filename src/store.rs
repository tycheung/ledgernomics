//! File store under `.ledgernomics/` (YAML SSOT).

use crate::schema::{Project, Slice, SliceStatus};
use crate::util::truncate;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

const DIR: &str = ".ledgernomics";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("slice not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProgressFile {
    #[serde(default = "none_slice")]
    current_slice: String,
    #[serde(default = "idle_status")]
    status: String,
    #[serde(default = "none_blockers")]
    blockers: String,
}

impl Default for ProgressFile {
    fn default() -> Self {
        Self {
            current_slice: none_slice(),
            status: idle_status(),
            blockers: none_blockers(),
        }
    }
}

fn none_slice() -> String {
    "(none)".into()
}
fn idle_status() -> String {
    "idle".into()
}
fn none_blockers() -> String {
    "none".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub id: String,
    pub slice_id: String,
    pub summary: String,
    pub failure_mode: String,
    pub do_not_retry_without: String,
    pub created_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdrSummary {
    pub id: String,
    pub path: String,
    pub title: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerStats {
    pub total_slices: usize,
    pub open: usize,
    pub done: usize,
    pub blocked: usize,
    pub median_max_files_on_done: Option<f64>,
    pub note: String,
}

#[derive(Debug, Clone)]
pub struct LedgerStore {
    root: PathBuf,
}

impl LedgerStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn ledger_dir(&self) -> PathBuf {
        self.root.join(DIR)
    }

    pub fn ensure_layout(&self) -> Result<(), StoreError> {
        let d = self.ledger_dir();
        fs::create_dir_all(d.join("attempts"))?;
        fs::create_dir_all(d.join("adr"))?;
        write_if_missing(
            &d.join("PROJECT.yaml"),
            &serde_yaml::to_string(&Project::default())?,
        )?;
        write_if_missing(&d.join("SLICE_BACKLOG.yaml"), "slices: []\n")?;
        write_if_missing(
            &d.join("PROGRESS.yaml"),
            &serde_yaml::to_string(&ProgressFile::default())?,
        )?;
        Ok(())
    }

    pub fn load_project(&self) -> Result<Project, StoreError> {
        self.ensure_layout()?;
        Ok(serde_yaml::from_str(&fs::read_to_string(
            self.ledger_dir().join("PROJECT.yaml"),
        )?)?)
    }

    pub fn save_project(&self, project: &Project) -> Result<(), StoreError> {
        self.ensure_layout()?;
        fs::write(
            self.ledger_dir().join("PROJECT.yaml"),
            serde_yaml::to_string(project)?,
        )?;
        Ok(())
    }

    pub fn load_slices(&self) -> Result<Vec<Slice>, StoreError> {
        self.ensure_layout()?;
        #[derive(Deserialize)]
        struct File {
            #[serde(default)]
            slices: Vec<Slice>,
        }
        let file: File = serde_yaml::from_str(&fs::read_to_string(
            self.ledger_dir().join("SLICE_BACKLOG.yaml"),
        )?)?;
        Ok(file.slices)
    }

    pub fn save_slices(&self, slices: &[Slice]) -> Result<(), StoreError> {
        self.ensure_layout()?;
        #[derive(Serialize)]
        struct File<'a> {
            slices: &'a [Slice],
        }
        fs::write(
            self.ledger_dir().join("SLICE_BACKLOG.yaml"),
            serde_yaml::to_string(&File { slices })?,
        )?;
        Ok(())
    }

    pub fn upsert_slice(&self, slice: Slice) -> Result<Slice, StoreError> {
        self.upsert_slice_ex(slice, false)
    }

    pub fn upsert_slice_ex(
        &self,
        mut slice: Slice,
        status_explicit: bool,
    ) -> Result<Slice, StoreError> {
        let mut slices = self.load_slices()?;
        if let Some(pos) = slices.iter().position(|s| s.id == slice.id) {
            if !status_explicit {
                slice.status = slices[pos].status.clone();
            }
            slices[pos] = slice.clone();
        } else {
            slices.push(slice.clone());
        }
        self.save_slices(&slices)?;
        self.ensure_progress_points_at_open(&slices)?;
        Ok(slice)
    }

    pub fn next_open_slice(&self) -> Result<Option<Slice>, StoreError> {
        Ok(self
            .load_slices()?
            .into_iter()
            .find(|s| s.status == SliceStatus::Open))
    }

    pub fn complete_slice(&self, id: &str) -> Result<Slice, StoreError> {
        let mut slices = self.load_slices()?;
        let pos = slices
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        slices[pos].status = SliceStatus::Done;
        let done = slices[pos].clone();
        self.save_slices(&slices)?;
        let next = slices
            .iter()
            .find(|s| s.status == SliceStatus::Open)
            .map(|s| s.id.as_str())
            .unwrap_or("(none)");
        self.write_progress(ProgressFile {
            current_slice: next.into(),
            status: format!("advanced after completing `{id}`"),
            blockers: none_blockers(),
        })?;
        Ok(done)
    }

    pub fn read_progress(&self) -> Result<String, StoreError> {
        let p = self.load_progress()?;
        Ok(format!(
            "current_slice: {}\nstatus: {}\nblockers: {}\n",
            p.current_slice, p.status, p.blockers
        ))
    }

    pub fn current_slice(&self) -> Result<Option<Slice>, StoreError> {
        let p = self.load_progress()?;
        if p.current_slice != "(none)" {
            if let Some(s) = self
                .load_slices()?
                .into_iter()
                .find(|s| s.id == p.current_slice)
            {
                return Ok(Some(s));
            }
        }
        self.next_open_slice()
    }

    pub fn append_attempt(
        &self,
        slice_id: &str,
        summary: &str,
        failure_mode: &str,
        do_not_retry_without: &str,
    ) -> Result<Attempt, StoreError> {
        self.ensure_layout()?;
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let id = format!("att-{created}-{nanos}");
        let attempt = Attempt {
            id: id.clone(),
            slice_id: slice_id.to_string(),
            summary: summary.to_string(),
            failure_mode: failure_mode.to_string(),
            do_not_retry_without: do_not_retry_without.to_string(),
            created_unix: created,
        };
        fs::write(
            self.ledger_dir()
                .join("attempts")
                .join(format!("{id}.yaml")),
            serde_yaml::to_string(&attempt)?,
        )?;
        Ok(attempt)
    }

    pub fn list_attempts(&self) -> Result<Vec<Attempt>, StoreError> {
        self.ensure_layout()?;
        let mut out = Vec::new();
        for path in list_ext(&self.ledger_dir().join("attempts"), "yaml")? {
            if let Ok(a) = serde_yaml::from_str::<Attempt>(&fs::read_to_string(&path)?) {
                out.push(a);
            }
        }
        out.sort_by_key(|b| std::cmp::Reverse(b.created_unix));
        Ok(out)
    }

    pub fn list_adrs(&self) -> Result<Vec<AdrSummary>, StoreError> {
        self.ensure_layout()?;
        let mut out = Vec::new();
        for path in list_ext(&self.ledger_dir().join("adr"), "md")? {
            let text = fs::read_to_string(&path)?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("adr")
                .to_string();
            out.push(adr_summary(&name, &text, 400));
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    pub fn get_adr(&self, id: &str) -> Result<AdrSummary, StoreError> {
        let id = id.trim().trim_end_matches(".md");
        let path = self.ledger_dir().join("adr").join(format!("{id}.md"));
        let text = fs::read_to_string(&path).map_err(|_| StoreError::NotFound(id.to_string()))?;
        Ok(adr_summary(id, &text, 800))
    }

    pub fn stats(&self) -> Result<LedgerStats, StoreError> {
        let slices = self.load_slices()?;
        let mut open = 0;
        let mut done = 0;
        let mut blocked = 0;
        let mut done_files = Vec::new();
        for s in &slices {
            match s.status {
                SliceStatus::Open => open += 1,
                SliceStatus::Done => {
                    done += 1;
                    done_files.push(s.max_files);
                }
                SliceStatus::Blocked => blocked += 1,
            }
        }
        Ok(LedgerStats {
            total_slices: slices.len(),
            open,
            done,
            blocked,
            median_max_files_on_done: median_u32(&done_files),
            note: "median_max_files_on_done uses declared max_files, not measured diff.".into(),
        })
    }

    fn load_progress(&self) -> Result<ProgressFile, StoreError> {
        self.ensure_layout()?;
        Ok(serde_yaml::from_str(&fs::read_to_string(
            self.ledger_dir().join("PROGRESS.yaml"),
        )?)?)
    }

    fn write_progress(&self, p: ProgressFile) -> Result<(), StoreError> {
        fs::write(
            self.ledger_dir().join("PROGRESS.yaml"),
            serde_yaml::to_string(&p)?,
        )?;
        Ok(())
    }

    fn ensure_progress_points_at_open(&self, slices: &[Slice]) -> Result<(), StoreError> {
        let p = self.load_progress()?;
        let open = slices.iter().find(|s| s.status == SliceStatus::Open);
        let needs = p.current_slice == "(none)"
            || !slices
                .iter()
                .any(|s| s.id == p.current_slice && s.status == SliceStatus::Open);
        if needs {
            self.write_progress(ProgressFile {
                current_slice: open.map(|s| s.id.clone()).unwrap_or_else(none_slice),
                status: if open.is_some() {
                    "tracking open slice".into()
                } else {
                    idle_status()
                },
                blockers: none_blockers(),
            })?;
        }
        Ok(())
    }
}

fn write_if_missing(path: &Path, body: &str) -> Result<(), StoreError> {
    if !path.exists() {
        fs::write(path, body)?;
    }
    Ok(())
}

fn list_ext(dir: &Path, ext: &str) -> Result<Vec<PathBuf>, StoreError> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(path);
        }
    }
    Ok(out)
}

fn adr_summary(id: &str, text: &str, max: usize) -> AdrSummary {
    let title = text
        .lines()
        .find_map(|l| l.strip_prefix("# ").map(|s| s.trim().to_string()))
        .unwrap_or_else(|| id.to_string());
    AdrSummary {
        id: id.to_string(),
        path: format!(".ledgernomics/adr/{id}.md"),
        title,
        excerpt: truncate(text, max),
    }
}

fn median_u32(vals: &[u32]) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    let mut v = vals.to_vec();
    v.sort_unstable();
    let n = v.len();
    Some(if n % 2 == 1 {
        v[n / 2] as f64
    } else {
        (v[n / 2 - 1] as f64 + v[n / 2] as f64) / 2.0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample(status: SliceStatus) -> Slice {
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
            status,
            blockers: String::new(),
        }
    }

    #[test]
    fn roundtrip_and_progress_on_upsert() {
        let dir = tempdir().unwrap();
        let store = LedgerStore::new(dir.path());
        store
            .save_project(&Project {
                goals: "ship ledger".into(),
                non_goals: "maker".into(),
                status: "active".into(),
            })
            .unwrap();
        assert_eq!(store.load_project().unwrap().goals, "ship ledger");
        store.upsert_slice(sample(SliceStatus::Open)).unwrap();
        let cur = store.current_slice().unwrap().unwrap();
        assert_eq!(cur.id, "led001");
        store.complete_slice("led001").unwrap();
        assert!(store.next_open_slice().unwrap().is_none());
    }

    #[test]
    fn upsert_preserves_done_status() {
        let dir = tempdir().unwrap();
        let store = LedgerStore::new(dir.path());
        store.upsert_slice(sample(SliceStatus::Open)).unwrap();
        store.complete_slice("led001").unwrap();
        let mut again = sample(SliceStatus::Open);
        again.title = "retitled".into();
        let saved = store.upsert_slice_ex(again, false).unwrap();
        assert_eq!(saved.status, SliceStatus::Done);
        assert_eq!(saved.title, "retitled");
        let reopened = store
            .upsert_slice_ex(sample(SliceStatus::Open), true)
            .unwrap();
        assert_eq!(reopened.status, SliceStatus::Open);
    }
}
