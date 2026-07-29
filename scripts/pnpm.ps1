param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$PnpmArguments
)

$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $PSScriptRoot
$env:COREPACK_HOME = Join-Path $projectRoot ".corepack"
$env:COREPACK_ENABLE_DOWNLOAD_PROMPT = "0"
$env:COREPACK_DEFAULT_TO_LATEST = "0"

$corepack = Get-Command corepack.cmd -ErrorAction SilentlyContinue

if (-not $corepack) {
    throw "Corepack est introuvable. Installe Node.js avec Corepack, puis relance cette commande."
}

Set-Location -LiteralPath $projectRoot
& $corepack.Source pnpm @PnpmArguments

if ($LASTEXITCODE -ne 0) {
    throw "pnpm a terminé avec le code $LASTEXITCODE."
}
