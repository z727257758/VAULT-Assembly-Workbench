[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ModelRoot,
    [Parameter(Mandatory = $true)][string]$PackingFixture,
    [string]$OpenScad = '',
    [ValidateRange(1, 8)][int]$TestThreads = 2
)
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$modelPath = (Resolve-Path -LiteralPath $ModelRoot).Path
$fixturePath = (Resolve-Path -LiteralPath $PackingFixture).Path
if (-not (Test-Path -LiteralPath $modelPath -PathType Container)) { throw 'ModelRoot must be a directory' }
if (-not (Test-Path -LiteralPath $fixturePath -PathType Leaf)) { throw 'PackingFixture must be a file' }
if ($OpenScad) {
    $scadPath = (Resolve-Path -LiteralPath $OpenScad).Path
} else {
    $scadPath = Join-Path $workspace 'tools/openscad/openscad-2021.01/openscad.com'
}
if (-not (Test-Path -LiteralPath $scadPath -PathType Leaf)) { throw 'OpenSCAD was not found; use -OpenScad or tools/setup-gridfinity.ps1' }
$testEnv = @{
    VAULT_MODEL_ROOT = $modelPath
    VAULT_EXPORT_TEST_ROOT = $modelPath
    VAULT_PACKING_FIXTURE = $fixturePath
    VAULT_OPENSCAD = $scadPath
    VAULT_GRIDFINITY_TEST = '1'
    VAULT_GRIDFINITY_BENCH = '1'
}
$previousEnv = @{}
foreach ($name in $testEnv.Keys) {
    $previousEnv[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
Push-Location -LiteralPath $workspace
try {
    foreach ($name in $testEnv.Keys) {
        [Environment]::SetEnvironmentVariable($name, $testEnv[$name], 'Process')
    }
    & cargo test --locked -- --ignored --skip contact_audit::dump_contact_regions --test-threads $TestThreads
    if ($LASTEXITCODE -ne 0) { throw "Integration tests failed (exit $LASTEXITCODE)" }
} finally {
    foreach ($name in $testEnv.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previousEnv[$name], 'Process')
    }
    Pop-Location
}
