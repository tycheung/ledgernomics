use ledgernomics::schema::{Project, Slice, SliceStatus};
use ledgernomics::LedgerStore;
use tempfile::tempdir;

#[test]
fn polish_attempts_adr_stats_wired() {
    let dir = tempdir().unwrap();
    let store = LedgerStore::new(dir.path());
    store
        .save_project(&Project {
            goals: "g".into(),
            non_goals: "n".into(),
            status: "active".into(),
        })
        .unwrap();
    store
        .upsert_slice(Slice {
            id: "s1".into(),
            title: "t".into(),
            goal: "goal".into(),
            target_paths: vec!["a.rs".into()],
            acceptance: "ok".into(),
            out_of_scope: "ui".into(),
            tests: vec![],
            max_files: 3,
            max_loc: 120,
            status: SliceStatus::Open,
            blockers: String::new(),
        })
        .unwrap();
    let a = store
        .append_attempt("s1", "tried X", "compile_fail", "fix deps")
        .unwrap();
    assert!(a.id.starts_with("att-"));
    assert_eq!(store.list_attempts().unwrap().len(), 1);
    std::fs::write(
        store.ledger_dir().join("adr").join("001-demo.md"),
        "# Demo ADR\n\nBody here.\n",
    )
    .unwrap();
    let ads = store.list_adrs().unwrap();
    assert_eq!(ads.len(), 1);
    assert_eq!(ads[0].title, "Demo ADR");
    assert_eq!(store.get_adr("001-demo").unwrap().id, "001-demo");
    store.complete_slice("s1").unwrap();
    let st = store.stats().unwrap();
    assert_eq!(st.done, 1);
    assert_eq!(st.open, 0);
    assert_eq!(st.median_max_files_on_done, Some(3.0));
}
