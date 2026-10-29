$LentorArguments = $args

$ErrorActionPreference = 'Stop'
$engine = $env:LENTOR_BINARY
if (-not $engine) {
    $engine = Join-Path $PSScriptRoot 'codex-rs\target\debug\codex.exe'
}
if (-not (Test-Path -LiteralPath $engine -PathType Leaf)) {
    throw 'Build the local engine with cargo build -p codex-cli --bin codex from codex-rs, or set LENTOR_BINARY to its absolute path.'
}
$engine = (Resolve-Path -LiteralPath $engine).ProviderPath
$userHome = [Environment]::GetFolderPath('UserProfile')
$lentorHome = $env:LENTOR_HOME
if (-not $lentorHome) {
    $lentorHome = Join-Path $userHome '.lentor'
}
$lentorHome = [IO.Path]::GetFullPath($lentorHome)
$upstreamHome = [IO.Path]::GetFullPath((Join-Path $userHome '.codex'))
if ($lentorHome.TrimEnd('\', '/') -eq $upstreamHome.TrimEnd('\', '/')) {
    throw 'LENTOR_HOME must be separate from the existing Codex profile.'
}
[void][IO.Directory]::CreateDirectory($lentorHome)

# Bridge the existing engine's home setting only for this invocation.
$previousHome = [Environment]::GetEnvironmentVariable('CODEX_HOME', 'Process')
try {
    $env:CODEX_HOME = $lentorHome
    & $engine --no-daemon @LentorArguments
    $lentorExitCode = $LASTEXITCODE
}
finally {
    [Environment]::SetEnvironmentVariable('CODEX_HOME', $previousHome, 'Process')
}
if ($lentorExitCode -ne 0) {
    exit $lentorExitCode
}
