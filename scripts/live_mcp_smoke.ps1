# Live MCP stdio smoke against release binary (newline-delimited JSON-RPC).
$ErrorActionPreference = "Stop"
$exe = Join-Path $PSScriptRoot "..\target\release\ledgernomics.exe"
$root = Join-Path $PSScriptRoot "..\.run\live-e2e"
New-Item -ItemType Directory -Force -Path $root | Out-Null
if (-not (Test-Path $exe)) { throw "missing release binary; run cargo build --release" }

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $exe
$psi.Arguments = "--root `"$root`""
$psi.UseShellExecute = $false
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.CreateNoWindow = $true
$p = [System.Diagnostics.Process]::Start($psi)

function Send-Msg([hashtable]$obj) {
  $json = ($obj | ConvertTo-Json -Compress -Depth 20)
  $line = $json + "`n"
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($line)
  $p.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
  $p.StandardInput.BaseStream.Flush()
}

function Read-Msg {
  $line = $p.StandardOutput.ReadLine()
  if ($null -eq $line) { throw "EOF from MCP" }
  return $line | ConvertFrom-Json
}

try {
  Send-Msg @{
    jsonrpc = "2.0"; id = 1; method = "initialize"
    params = @{
      protocolVersion = "2024-11-05"
      capabilities = @{}
      clientInfo = @{ name = "ledgernomics-smoke"; version = "0.1" }
    }
  }
  $init = Read-Msg
  if (-not $init.result) { throw ("initialize failed: " + ($init | ConvertTo-Json -Compress)) }
  Send-Msg @{ jsonrpc = "2.0"; method = "notifications/initialized" }

  function Call-Tool($id, $name, $arguments) {
    Send-Msg @{
      jsonrpc = "2.0"; id = $id; method = "tools/call"
      params = @{ name = $name; arguments = $arguments }
    }
    $r = Read-Msg
    if ($r.error) { throw ("$name error: " + ($r.error | ConvertTo-Json -Compress)) }
    return $r.result
  }

  Call-Tool 2 "project_set" @{ goals = "live e2e"; non_goals = "maker"; status = "active" } | Out-Null
  Call-Tool 3 "project_get" @{} | Out-Null
  Write-Host "project_get ok"

  Call-Tool 4 "slice_upsert" @{
    id = "live1"; title = "t"; goal = "g"
    target_paths = @("src/a.rs"); acceptance = "ok"; out_of_scope = "ui"
  } | Out-Null
  Call-Tool 5 "slice_next" @{} | Out-Null
  Call-Tool 6 "recover_context" @{ budget_chars = 8000; shape = "steward" } | Out-Null
  Call-Tool 14 "assemble_context" @{ budget_chars = 8000; shape = "feature" } | Out-Null
  Call-Tool 15 "prefix_fingerprint" @{ shape = "feature" } | Out-Null
  Call-Tool 7 "attempt_append" @{
    slice_id = "live1"; summary = "try"; failure_mode = "x"; do_not_retry_without = "y"
  } | Out-Null
  Call-Tool 8 "attempt_list" @{} | Out-Null

  $adrDir = Join-Path $root ".ledgernomics\adr"
  New-Item -ItemType Directory -Force -Path $adrDir | Out-Null
  Set-Content (Join-Path $adrDir "001-live.md") "# Live ADR`n`nbody`n"
  Call-Tool 9 "adr_list" @{} | Out-Null
  Call-Tool 10 "adr_get" @{ id = "001-live" } | Out-Null
  Call-Tool 11 "slice_complete" @{ id = "live1" } | Out-Null

  Send-Msg @{ jsonrpc = "2.0"; id = 12; method = "resources/list"; params = @{} }
  $rl = Read-Msg
  if (-not $rl.result.resources) { throw "resources/list empty" }
  Send-Msg @{
    jsonrpc = "2.0"; id = 13; method = "resources/read"
    params = @{ uri = "ledger://project" }
  }
  $rr = Read-Msg
  if (-not $rr.result) { throw "resources/read failed" }

  Write-Host "LIVE_MCP_SMOKE_OK"
}
finally {
  if (-not $p.HasExited) { $p.Kill() }
}
