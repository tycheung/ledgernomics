use ledgernomics::schema::{Project, Slice, SliceStatus};
use ledgernomics::LedgerStore;
use tempfile::tempdir;

#[test]
fn ledger_roundtrip_reject_and_recover() {
    let dir = tempdir().unwrap();
    let store = LedgerStore::new(dir.path());
    store
        .save_project(&Project {
            goals: "v1".into(),
            non_goals: "maker".into(),
            status: "active".into(),
        })
        .unwrap();

    let bad = Slice {
        id: "x".into(),
        title: "t".into(),
        goal: "g".into(),
        target_paths: vec!["a.rs".into()],
        acceptance: "ok".into(),
        out_of_scope: "".into(),
        tests: vec![],
        max_files: 3,
        max_loc: 120,
        status: SliceStatus::Open,
        blockers: String::new(),
    };
    assert!(bad.validate().is_err());

    let good = Slice {
        out_of_scope: "ui".into(),
        ..bad
    };
    store.upsert_slice(good).unwrap();
    let next = store.next_open_slice().unwrap().unwrap();
    assert_eq!(next.id, "x");
    assert_eq!(next.out_of_scope, "ui");

    let progress = store.read_progress().unwrap();
    let pkt = ledgernomics::recover::recover_context(
        &store.load_project().unwrap(),
        Some(&next),
        &progress,
        ledgernomics::SessionShape::Steward,
        8000,
    );
    assert!(pkt.ok);
    assert!(pkt.stable.steward_priority.is_some());
    assert_eq!(
        pkt.stable.current_slice.as_ref().unwrap().out_of_scope,
        "ui"
    );
}
