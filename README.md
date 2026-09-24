# Ledgernomics

Stdio MCP that keeps project goals, non-goals, and budgeted micro-slices in
`.ledgernomics/` YAML, and returns cache-friendly context packets for coding agents.

## Tools

| Tool | Behavior |
|------|----------|
| `scope_epic` | Single-prompt entry: draft/validate goals, non_goals, slices; clarifications; `commit=true` writes ledger |
| `project_get` / `project_set` | Goals, non_goals, status (+ stats on get) |
| `slice_upsert` | Create/update; requires `acceptance`, `target_paths`, `out_of_scope` |
| `slice_next` / `slice_complete` | Next open slice; mark done |
| `assemble_context` / `prefix_fingerprint` | Stable/volatile packet + fingerprint; pin stable while fingerprint unchanged |
| `recover_context` | Same packet as `assemble_context` |
| `attempt_append` / `attempt_list` | Failed-attempt records |
| `adr_list` / `adr_get` | ADR pointers + short excerpts |

Resources: `ledger://project`, `ledger://slice/current`.

Default slice budgets: ≤3 files / ≤120 LOC.

## Run

```bash
cargo build --release
./target/release/ledgernomics --root /path/to/project
```

Cursor MCP:

```json
{
  "mcpServers": {
    "ledgernomics": {
      "type": "stdio",
      "command": "/absolute/path/to/target/release/ledgernomics",
      "args": ["--root", "/absolute/path/to/your/project"]
    }
  }
}
```

## Develop

```powershell
.\scripts\ci_check.ps1
```
