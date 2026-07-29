$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $projectRoot ".cargo-home"
$env:CARGO_BUILD_JOBS = "1"
$rustBin = Join-Path $env:USERPROFILE ".cargo\bin"
$tauriRunner = Join-Path $projectRoot "node_modules\.bin\tauri.cmd"

if (Test-Path -LiteralPath $rustBin) {
    $env:Path = "$rustBin;$env:Path"
}

Set-Location -LiteralPath $projectRoot
if (-not (Test-Path -LiteralPath $tauriRunner)) {
    throw "Les dépendances locales sont absentes. Exécute d'abord install.cmd."
}

& $tauriRunner dev

if ($LASTEXITCODE -ne 0) {
    throw "Tauri a terminé avec le code $LASTEXITCODE."
}
