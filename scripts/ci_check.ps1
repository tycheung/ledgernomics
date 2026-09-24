$ErrorActionPreference = "Stop"
$env:CARGO_BUILD_JOBS = "2"
Set-Location $PSScriptRoot\..

function Invoke-Step([string]$Name, [scriptblock]$Block) {
  Write-Host "== $Name =="
  & $Block
  if ($LASTEXITCODE -ne 0) {
    throw "$Name failed with exit $LASTEXITCODE"
  }
}

Invoke-Step "fmt" { cargo fmt --all -- --check }
Invoke-Step "clippy" { cargo clippy --all-targets -- -D warnings }
Invoke-Step "test" { cargo test }
Write-Host "ci_check OK"
