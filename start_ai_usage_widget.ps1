$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$electron = Join-Path $projectRoot "node_modules\electron\dist\electron.exe"

if (-not (Test-Path $electron)) {
    throw "Electron is not installed. Run 'npm install' in $projectRoot first."
}

Set-Location $projectRoot
& $electron "."
