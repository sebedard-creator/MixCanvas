$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$env:CARGO_HOME = Join-Path $projectRoot ".cargo-home"
$manifestPath = Join-Path $projectRoot "src-tauri\Cargo.toml"
$rustBin = Join-Path $env:USERPROFILE ".cargo\bin"
$typescriptRunner = Join-Path $projectRoot "node_modules\.bin\tsc.cmd"
$vitestRunner = Join-Path $projectRoot "node_modules\.bin\vitest.cmd"

if (Test-Path -LiteralPath $rustBin) {
    $env:Path = "$rustBin;$env:Path"
}

Set-Location -LiteralPath $projectRoot

if (-not (Test-Path -LiteralPath $typescriptRunner) -or -not (Test-Path -LiteralPath $vitestRunner)) {
    throw "Les dépendances locales sont absentes. Exécute d'abord install.cmd."
}

& $typescriptRunner --noEmit

if ($LASTEXITCODE -ne 0) {
    throw "TypeScript a échoué avec le code $LASTEXITCODE."
}

& $vitestRunner run

if ($LASTEXITCODE -ne 0) {
    throw "Vitest a échoué avec le code $LASTEXITCODE."
}

cargo test --manifest-path $manifestPath

if ($LASTEXITCODE -ne 0) {
    throw "Les tests Rust ont échoué avec le code $LASTEXITCODE."
}

cargo fmt --manifest-path $manifestPath --check

if ($LASTEXITCODE -ne 0) {
    throw "Le formatage Rust n'est pas conforme (code $LASTEXITCODE)."
}

cargo clippy --manifest-path $manifestPath --all-targets -- -D warnings

if ($LASTEXITCODE -ne 0) {
    throw "Clippy a échoué avec le code $LASTEXITCODE."
}
