//! Filesystem YAML ledger MCP for budgeted micro-slices and context recovery.

pub mod recover;
pub mod schema;
pub mod scope;
pub mod service;
pub mod store;
pub mod util;

pub use schema::{Project, SessionShape, Slice, SliceStatus};
pub use service::LedgerService;
pub use store::LedgerStore;
