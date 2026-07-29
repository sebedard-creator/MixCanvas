$ErrorActionPreference = "Stop"

$projectRoot = $PSScriptRoot
$checkRunner = Join-Path $projectRoot "scripts\check.ps1"

& $checkRunner
