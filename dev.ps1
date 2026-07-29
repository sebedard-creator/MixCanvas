$ErrorActionPreference = "Stop"

$projectRoot = $PSScriptRoot
$devRunner = Join-Path $projectRoot "scripts\dev.ps1"

& $devRunner
