//! MCP tool surface (rmcp).

use crate::recover::{assemble_context, build_stable, fingerprint_stable, VolatileInputs};
use crate::schema::{
    Project, SessionShape, Slice, SliceStatus, DEFAULT_MAX_FILES, DEFAULT_MAX_LOC,
};
use crate::scope::{project_from_scope, scope_epic, ScopeInput, ScopeSliceDraft};
use crate::store::{LedgerStore, StoreError};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    Implementation, ListResourcesResult, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Clone)]
pub struct LedgerService {
    store: Arc<LedgerStore>,
}

fn json_ok<T: serde::Serialize>(v: &T) -> Result<String, McpError> {
    serde_json::to_string_pretty(v).map_err(|e| McpError::internal_error(e.to_string(), None))
}

fn map_store(e: StoreError) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectSetArgs {
    pub goals: String,
    pub non_goals: String,
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    "active".into()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SliceUpsertArgs {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub goal: String,
    pub target_paths: Vec<String>,
    pub acceptance: String,
    pub out_of_scope: String,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default = "default_files")]
    pub max_files: u32,
    #[serde(default = "default_loc")]
    pub max_loc: u32,
    #[serde(default)]
    pub blockers: String,
    #[serde(default)]
    pub status: String,
}

fn default_files() -> u32 {
    DEFAULT_MAX_FILES
}
fn default_loc() -> u32 {
    DEFAULT_MAX_LOC
}

impl From<SliceUpsertArgs> for Slice {
    fn from(a: SliceUpsertArgs) -> Self {
        Self {
            id: a.id,
            title: a.title,
            goal: a.goal,
            target_paths: a.target_paths,
            acceptance: a.acceptance,
            out_of_scope: a.out_of_scope,
            tests: a.tests,
            max_files: a.max_files,
            max_loc: a.max_loc,
            status: SliceStatus::from_str(&a.status).unwrap_or(SliceStatus::Open),
            blockers: a.blockers,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SliceIdArgs {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecoverArgs {
    #[serde(default = "default_budget")]
    pub budget_chars: usize,
    #[serde(default = "default_feature")]
    pub shape: String,
}

fn default_budget() -> usize {
    4000
}
fn default_feature() -> String {
    "feature".into()
}

fn assemble_packet(store: &LedgerStore, args: &RecoverArgs) -> Result<String, McpError> {
    let shape = SessionShape::from_str(&args.shape).unwrap_or(SessionShape::Feature);
    let project = store.load_project().map_err(map_store)?;
    let current = store.current_slice().map_err(map_store)?;
    let progress = store.read_progress().map_err(map_store)?;
    let attempts = store.list_attempts().map_err(map_store)?;
    let stats = store.stats().map_err(map_store)?;
    let latest = attempts.first();
    json_ok(&assemble_context(
        &project,
        current.as_ref(),
        shape,
        args.budget_chars,
        VolatileInputs {
            progress: &progress,
            latest_attempt: latest,
            stats: Some(&stats),
        },
    ))
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AttemptAppendArgs {
    pub slice_id: String,
    pub summary: String,
    pub failure_mode: String,
    #[serde(default)]
    pub do_not_retry_without: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AdrIdArgs {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScopeEpicArgs {
    pub brief: String,
    #[serde(default)]
    pub goals: String,
    #[serde(default)]
    pub non_goals: String,
    #[serde(default)]
    pub slices: Vec<ScopeSliceDraft>,
    #[serde(default)]
    pub commit: bool,
}

#[tool_router]
impl LedgerService {
    pub fn new(root: PathBuf) -> Self {
        Self {
            store: Arc::new(LedgerStore::new(root)),
        }
    }

    #[tool(
        description = "Single-prompt epic entry: draft/validate goals, non_goals, slices; return clarifications to ask the user; commit=true writes ledger when ok."
    )]
    async fn scope_epic(
        &self,
        Parameters(args): Parameters<ScopeEpicArgs>,
    ) -> Result<String, McpError> {
        let commit = args.commit;
        let mut result = scope_epic(ScopeInput {
            brief: args.brief,
            goals: args.goals,
            non_goals: args.non_goals,
            slices: args.slices,
            commit,
        });
        if result.ok && commit {
            self.store
                .save_project(&project_from_scope(&result))
                .map_err(map_store)?;
            for slice in &result.slices {
                self.store
                    .upsert_slice_ex(slice.clone(), true)
                    .map_err(map_store)?;
            }
            result.committed = true;
            result.status = "committed".into();
            result.message =
                "Ledger written. Use assemble_context / prefix_fingerprint; pin stable until fingerprint changes."
                    .into();
        } else if commit && !result.ok {
            result.message =
                "commit=true ignored: resolve clarifications (ask the user) then retry.".into();
        }
        json_ok(&result)
    }

    #[tool(description = "Get project goals, non_goals, status, and slice stats")]
    async fn project_get(&self) -> Result<String, McpError> {
        let p = self.store.load_project().map_err(map_store)?;
        let stats = self.store.stats().map_err(map_store)?;
        json_ok(&json!({ "project": p, "stats": stats }))
    }

    #[tool(description = "Set project goals, non_goals, and status")]
    async fn project_set(
        &self,
        Parameters(args): Parameters<ProjectSetArgs>,
    ) -> Result<String, McpError> {
        let p = Project {
            goals: args.goals,
            non_goals: args.non_goals,
            status: args.status,
        };
        self.store.save_project(&p).map_err(map_store)?;
        json_ok(&p)
    }

    #[tool(
        description = "Create/update a micro-slice; requires acceptance, target_paths, out_of_scope"
    )]
    async fn slice_upsert(
        &self,
        Parameters(args): Parameters<SliceUpsertArgs>,
    ) -> Result<String, McpError> {
        let s = args.status.trim();
        let explicit = if s.is_empty() {
            false
        } else if SliceStatus::from_str(s).is_err() {
            return Err(McpError::invalid_params(
                format!("status must be open|blocked|done, got {s}"),
                Some(json!({"code": "validation"})),
            ));
        } else {
            true
        };
        let slice = Slice::from(args);
        slice.validate().map_err(|e| {
            McpError::invalid_params(e.to_string(), Some(json!({"code": "validation"})))
        })?;
        json_ok(
            &self
                .store
                .upsert_slice_ex(slice, explicit)
                .map_err(map_store)?,
        )
    }

    #[tool(description = "Return the next open slice")]
    async fn slice_next(&self) -> Result<String, McpError> {
        json_ok(&self.store.next_open_slice().map_err(map_store)?)
    }

    #[tool(description = "Mark a slice done and advance PROGRESS.yaml")]
    async fn slice_complete(
        &self,
        Parameters(args): Parameters<SliceIdArgs>,
    ) -> Result<String, McpError> {
        json_ok(&self.store.complete_slice(&args.id).map_err(map_store)?)
    }

    #[tool(
        description = "Cache-aware context assembly: stable vs volatile tiers + fingerprint + host_hint. shape=feature|steward."
    )]
    async fn assemble_context(
        &self,
        Parameters(args): Parameters<RecoverArgs>,
    ) -> Result<String, McpError> {
        assemble_packet(&self.store, &args)
    }

    #[tool(
        description = "Budgeted context recovery (same packet as assemble_context). Critical fields never silently dropped."
    )]
    async fn recover_context(
        &self,
        Parameters(args): Parameters<RecoverArgs>,
    ) -> Result<String, McpError> {
        assemble_packet(&self.store, &args)
    }

    #[tool(
        description = "Return fingerprint of stable prefix only (goals/slice-critical/steward). Cheap cache invalidation check."
    )]
    async fn prefix_fingerprint(
        &self,
        Parameters(args): Parameters<RecoverArgs>,
    ) -> Result<String, McpError> {
        let shape = SessionShape::from_str(&args.shape).unwrap_or(SessionShape::Feature);
        let shape_s = shape.as_str();
        let project = self.store.load_project().map_err(map_store)?;
        let current = self.store.current_slice().map_err(map_store)?;
        let stable = build_stable(&project, current.as_ref(), shape);
        let fingerprint = fingerprint_stable(&stable);
        json_ok(&json!({
            "fingerprint": fingerprint,
            "shape": shape_s,
            "host_hint": crate::recover::HOST_HINT,
        }))
    }

    #[tool(description = "Append a failed-attempt record for a slice")]
    async fn attempt_append(
        &self,
        Parameters(args): Parameters<AttemptAppendArgs>,
    ) -> Result<String, McpError> {
        json_ok(
            &self
                .store
                .append_attempt(
                    &args.slice_id,
                    &args.summary,
                    &args.failure_mode,
                    &args.do_not_retry_without,
                )
                .map_err(map_store)?,
        )
    }

    #[tool(description = "List attempt/experiment records (newest first)")]
    async fn attempt_list(&self) -> Result<String, McpError> {
        json_ok(&self.store.list_attempts().map_err(map_store)?)
    }

    #[tool(description = "List ADR markdown files as id/path/title/excerpt")]
    async fn adr_list(&self) -> Result<String, McpError> {
        json_ok(&self.store.list_adrs().map_err(map_store)?)
    }

    #[tool(description = "Get one ADR by id (short excerpt, not full paste)")]
    async fn adr_get(&self, Parameters(args): Parameters<AdrIdArgs>) -> Result<String, McpError> {
        json_ok(&self.store.get_adr(&args.id).map_err(map_store)?)
    }
}

#[tool_handler]
impl ServerHandler for LedgerService {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "ledgernomics",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Ledgernomics v5 — single-prompt entry via scope_epic, then cache-aware assemble_context. \
On one epic user prompt: scope_epic(brief, drafted goals/non_goals/slices). If clarifications non-empty, ask the user—do not commit. \
When ok, scope_epic(..., commit=true). Each slice: prefix_fingerprint/assemble_context; pin stable until fingerprint changes. \
slice_upsert requires out_of_scope.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                Resource::new("ledger://project", "project")
                    .with_title("Project goals")
                    .with_description("PROJECT.yaml projection")
                    .with_mime_type("application/json"),
                Resource::new("ledger://slice/current", "current_slice")
                    .with_title("Current slice")
                    .with_description("Current or next open slice")
                    .with_mime_type("application/json"),
            ],
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let uri = request.uri.clone();
        let text = match uri.as_str() {
            "ledger://project" => json_ok(&self.store.load_project().map_err(map_store)?)?,
            "ledger://slice/current" => json_ok(&self.store.current_slice().map_err(map_store)?)?,
            other => {
                return Err(McpError::resource_not_found(
                    format!("unknown resource {other}"),
                    None,
                ));
            }
        };
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, uri)]).into())
    }
}
