# Opens the focused workspace's card in VS Code (Windows). `twig code` without an id reads
# the card from HERDR_PLUGIN_CONTEXT_JSON (workspace label, else pane cwd); outside a card
# it opens that folder. It also strips HERDR_* so VS Code's terminal can run herdr.
$ErrorActionPreference = 'Stop'

# the herdr server's PATH may predate the install: fall back to install.ps1's folder
$twig = (Get-Command twig -ErrorAction SilentlyContinue).Source
if (-not $twig) {
    $candidate = Join-Path $env:LOCALAPPDATA 'Programs\twig\twig.exe'
    if (Test-Path $candidate) { $twig = $candidate }
}
if (-not $twig) {
    [Console]::Error.WriteLine("twig not found in the herdr server's PATH (nor in $env:LOCALAPPDATA\Programs\twig)")
    exit 1
}

& $twig code
exit $LASTEXITCODE
