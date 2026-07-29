$ErrorActionPreference = "Stop"

$projectRoot = $PSScriptRoot
$pnpmRunner = Join-Path $projectRoot "scripts\pnpm.ps1"

& $pnpmRunner install
