# twig installer for Windows (native; on WSL use install.sh instead).
#
#   irm https://github.com/RafaelKC/twig/releases/latest/download/install.ps1 | iex
#
# Checks/installs the dependencies (docs/DEPENDENCIES.md) with winget and the official
# installers, downloads the twig release for Windows, verifies its checksum (and Sigstore
# signature when cosign is available), and installs the binary, the AI skill, the herdr plugin
# and PowerShell completions. Running it again updates an existing install.
#
# herdr's native Windows support is in BETA: twig works with it, but expect rough edges.
#
# No admin needed for twig itself: everything goes to per-user folders and the user PATH.
# Missing dependencies are installed per user when the package allows it (Git, VS Code, gh,
# herdr, Claude Code); the Azure CLI is machine-wide only, so installing it asks for admin
# approval (UAC).
#
# Options (when run as a file: .\install.ps1 -Update):
#   -Version vX.Y.Z   install that release (default: latest)
#   -Update           only update twig, skill, plugin and completions (no dependency setup)
#   -BinDir DIR       where to put twig.exe (default: existing install, else
#                     %LOCALAPPDATA%\Programs\twig)
#   -Yes              accept defaults without asking
#
# Env (also for `irm | iex`, which can't pass options): TWIG_REPO (owner/repo), TWIG_VERSION,
# TWIG_BIN_DIR, TWIG_CONFIG, TWIG_UPDATE=1, TWIG_YES=1,
# TWIG_DIST_DIR (use local release files instead of downloading; for testing).
#
# ASCII only on purpose: Windows PowerShell 5.1 reads a BOM-less script as ANSI.

param(
    [string]$Version = $env:TWIG_VERSION,
    [switch]$Update,
    [string]$BinDir = $env:TWIG_BIN_DIR,
    [switch]$Yes
)

function Install-Twig {
    param([string]$Version, [bool]$Update, [string]$BinDir, [bool]$Yes)

    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is very slow with the bar
    # Windows PowerShell 5.1 may default to TLS 1.0
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $Repo = if ($env:TWIG_REPO) { $env:TWIG_REPO } else { 'RafaelKC/twig' }
    $Mode = if ($Update -or $env:TWIG_UPDATE -eq '1') { 'update' } else { 'install' }
    $script:AssumeYes = $Yes -or $env:TWIG_YES -eq '1'
    $DataDir = Join-Path $env:LOCALAPPDATA 'twig'
    $ConfigFile = if ($env:TWIG_CONFIG) { $env:TWIG_CONFIG } else { Join-Path $env:APPDATA 'twig\config.toml' }
    $HerdrConfig = if ($env:HERDR_CONFIG_PATH) { $env:HERDR_CONFIG_PATH } else { Join-Path $env:APPDATA 'herdr\config.toml' }
    $Target = 'x86_64-pc-windows-msvc'

    # ------------------------------ output and prompts ------------------------------

    function Info($m) { Write-Host "-> $m" -ForegroundColor Cyan }
    function Ok($m) { Write-Host "ok $m" -ForegroundColor Green }
    function Warn($m) { Write-Host "!  $m" -ForegroundColor Yellow }
    function Die($m) { throw "twig installer: $m" }
    function Section($m) { Write-Host ""; Write-Host $m -ForegroundColor White }

    function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

    function Ask($prompt, $default) {
        if ($script:AssumeYes -or -not [Environment]::UserInteractive) { return $default }
        try { $a = Read-Host $prompt } catch { return $default }
        if ([string]::IsNullOrWhiteSpace($a)) { return $default }
        return $a.Trim()
    }

    function Confirm($question, $default) {
        $hint = if ($default -eq 'y') { '[Y/n]' } else { '[y/N]' }
        return (Ask "$question $hint" $default) -match '^(y|yes|s|sim)$'
    }

    # PATH as the registry has it now (installers update it, this session doesn't see it)
    function Refresh-Path {
        $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
        $user = [Environment]::GetEnvironmentVariable('Path', 'User')
        $env:Path = (@($machine, $user) | Where-Object { $_ }) -join ';'
    }

    # Per-user install first (Git, VS Code and the GitHub CLI's portable build have one: no
    # admin, no UAC prompt); only a package that ships machine-wide only falls back to the
    # default, and Windows then asks for admin approval.
    function Winget-Install($id) {
        if (-not (Have winget)) { Die "winget not found: install '$id' manually (or get App Installer from the Microsoft Store)" }
        $common = @('install', '--id', $id, '-e', '--source', 'winget', '--accept-package-agreements', '--accept-source-agreements')
        winget @common --scope user
        if ($LASTEXITCODE -ne 0) {
            Warn "$id has no per-user installer: Windows will ask for admin approval (UAC)"
            winget @common
        }
        Refresh-Path
    }

    # official installers that are themselves `irm | iex` scripts
    function Run-Remote-Installer($url) {
        powershell -NoProfile -ExecutionPolicy Bypass -Command "irm $url | iex"
        Refresh-Path
    }

    # ------------------------------ dependencies ------------------------------

    $script:AI = ''
    $script:VscodeEnabled = 'false'
    $script:Provider = ''
    $script:AzOrg = ''

    function Setup-Git {
        Section 'git'
        if (Have git) {
            Ok (git --version)
        } elseif (Confirm 'git not found (required). Install it?' 'y') {
            Winget-Install 'Git.Git'
            if (-not (Have git)) { Die 'git was installed but is not on PATH; open a new terminal and run the installer again' }
            Ok 'git installed'
        } else {
            Die 'git is required'
        }
        $name = git config --global user.name
        $email = git config --global user.email
        if ($name -and $email) {
            Ok "git identity: $name <$email>"
        } elseif (Confirm 'git user.name/user.email not set. Set them now?' 'y') {
            if (-not $name) { $name = Ask 'name:' '' }
            if (-not $email) { $email = Ask 'email:' '' }
            if ($name) { git config --global user.name $name }
            if ($email) { git config --global user.email $email }
            if ($name -and $email) { Ok 'git identity set' } else { Warn 'git identity incomplete (git config --global user.name/user.email)' }
        } else {
            Warn 'git identity not set (`twig doctor` will keep warning about it)'
        }
    }

    function Setup-Herdr {
        Section 'herdr (native Windows support is BETA)'
        if (Have herdr) {
            Ok (herdr --version)
        } elseif (Confirm 'herdr not found (required). Install it?' 'y') {
            Run-Remote-Installer 'https://herdr.dev/install.ps1'
            if (-not (Have herdr)) { Die 'herdr was installed but is not on PATH; open a new terminal and run the installer again' }
            Ok 'herdr installed'
        } else {
            Die 'herdr is required'
        }
    }

    function Setup-Vscode {
        Section 'VS Code (optional)'
        if (Have code) {
            Ok 'code found'
            $script:VscodeEnabled = 'true'
        } elseif (Confirm 'VS Code not found. Install it?' 'n') {
            Winget-Install 'Microsoft.VisualStudioCode'
            if (Have code) { Ok 'VS Code installed'; $script:VscodeEnabled = 'true' }
            else { Warn 'VS Code installed but `code` is not on PATH yet (open a new terminal)' }
        } else {
            Info 'continuing without VS Code (vscode.enabled = false)'
        }
    }

    function Check-Az-Login {
        az extension show --name azure-devops --output none 2>$null
        if ($LASTEXITCODE -ne 0) {
            Info 'adding the az azure-devops extension'
            az extension add --name azure-devops --only-show-errors
            if ($LASTEXITCODE -ne 0) { Warn 'could not add the azure-devops extension' }
        }
        $line = az devops configure --list 2>$null | Select-String '^organization\s*=\s*(.+)$'
        if ($line) { $script:AzOrg = $line.Matches[0].Groups[1].Value.Trim() }
        if (-not $script:AzOrg) {
            $script:AzOrg = Ask 'Azure DevOps organization (e.g. https://dev.azure.com/my-org, empty to skip):' ''
            if ($script:AzOrg) { az devops configure --defaults "organization=$($script:AzOrg)" }
        }
        az devops project list --top 1 --output none 2>$null
        if ($LASTEXITCODE -eq 0) {
            Ok 'az has access to Azure DevOps'
        } elseif (Confirm 'az has no access to Azure DevOps. Log in now?' 'y') {
            az login
            if ($LASTEXITCODE -ne 0) { Warn 'az login failed' }
        } else {
            Warn 'az not logged in (az login or az devops login)'
        }
    }

    function Check-Gh-Login {
        gh auth status 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) {
            Ok 'gh logged in'
        } elseif (Confirm 'gh is not logged in. Log in now?' 'y') {
            gh auth login
            if ($LASTEXITCODE -ne 0) { Warn 'gh login failed' }
        } else {
            Warn 'gh not logged in (gh auth login)'
        }
    }

    function Setup-Provider {
        Section 'card provider (az and/or gh)'
        $chosen = @()
        foreach ($p in @('az', 'gh')) {
            if ((Have $p) -and (Confirm "$p is already installed. Use it?" 'y')) { $chosen += $p }
        }
        if ($chosen.Count -eq 0) {
            $c = Ask 'choose a provider: [1] az (Azure DevOps) [2] gh (GitHub)' '1'
            if ($c -eq '2' -or $c -eq 'gh') { Winget-Install 'GitHub.cli'; $chosen = @('gh') }
            else {
                Warn 'the Azure CLI only installs machine-wide: Windows will ask for admin approval (UAC)'
                Winget-Install 'Microsoft.AzureCLI'; $chosen = @('az')
            }
        }
        Ok "provider: $($chosen -join ' ')"
        foreach ($p in $chosen) {
            if (-not $script:Provider) { $script:Provider = $p }
            if (-not (Have $p)) { Warn "$p is not on PATH yet (open a new terminal)"; continue }
            if ($p -eq 'az') { Check-Az-Login } else { Check-Gh-Login }
        }
    }

    function Setup-AI {
        Section 'AI'
        $choice = Ask 'which agent do you want to use? (claude/codex) [claude]:' 'claude'
        if ($choice -eq 'codex') {
            if (Have codex) {
                $script:AI = 'codex'; Ok 'using Codex'
            } elseif (Confirm 'Codex not found. Install it?' 'y') {
                if (Have npm) { npm install -g '@openai/codex'; Refresh-Path }
                else { Warn 'install Codex manually: https://github.com/openai/codex (needs Node.js/npm)' }
                if (Have codex) { $script:AI = 'codex'; Ok 'Codex installed' }
            }
        } else {
            if (Have claude) {
                if (Confirm 'Claude Code is already installed. Use it?' 'y') { $script:AI = 'claude'; Ok 'using Claude Code' }
            } elseif (Confirm 'Claude Code not found. Install it?' 'y') {
                Run-Remote-Installer 'https://claude.ai/install.ps1'
                if (Have claude) { $script:AI = 'claude'; Ok 'Claude Code installed' }
            }
        }
        if (-not $script:AI) { Warn 'no AI: the herdr agent tab will have nothing to run' }
    }

    # ------------------------------ download and verify ------------------------------

    function Download-Release {
        Section 'twig'
        if (-not $script:Version) {
            $rel = Invoke-RestMethod -UseBasicParsing "https://api.github.com/repos/$Repo/releases/latest"
            $script:Version = $rel.tag_name
            if (-not $script:Version) { Die "could not find the latest release of $Repo" }
        }
        $asset = "twig-$Target.zip"
        $base = "https://github.com/$Repo/releases/download/$($script:Version)"
        $script:Tmp = Join-Path ([IO.Path]::GetTempPath()) ("twig-install-" + [Guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $script:Tmp | Out-Null

        Info "downloading twig $($script:Version) ($Target)"
        foreach ($f in @($asset, 'SHA256SUMS', 'SHA256SUMS.sigstore.json')) {
            $out = Join-Path $script:Tmp $f
            if ($env:TWIG_DIST_DIR) {
                $src = Join-Path $env:TWIG_DIST_DIR $f
                if (Test-Path $src) { Copy-Item $src $out }
            } else {
                try { Invoke-WebRequest -UseBasicParsing -Uri "$base/$f" -OutFile $out }
                catch { if ($f -ne 'SHA256SUMS.sigstore.json') { Die "failed to download $base/$f" } }
            }
        }
        $zip = Join-Path $script:Tmp $asset
        if (-not (Test-Path $zip)) { Die "release $($script:Version) has no $asset" }

        $sums = Get-Content (Join-Path $script:Tmp 'SHA256SUMS')
        $expected = $null
        foreach ($l in $sums) {
            $parts = $l -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) { $expected = $parts[0] }
        }
        if (-not $expected) { Die "$asset is not in SHA256SUMS" }
        $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
        if ($actual -ne $expected.ToLower()) { Die "checksum mismatch for $asset" }
        Ok 'checksum verified'

        $bundle = Join-Path $script:Tmp 'SHA256SUMS.sigstore.json'
        if ((Test-Path $bundle) -and (Have cosign)) {
            cosign verify-blob --bundle $bundle `
                --certificate-identity-regexp "^https://github.com/$Repo/\.github/workflows/release\.yml@refs/tags/" `
                --certificate-oidc-issuer https://token.actions.githubusercontent.com `
                (Join-Path $script:Tmp 'SHA256SUMS') 2>$null | Out-Null
            if ($LASTEXITCODE -ne 0) { Die "invalid Sigstore signature for $($script:Version)" }
            Ok 'Sigstore signature verified'
        } else {
            Info 'cosign not installed: signature not verified (the checksum was). Manual verification in docs/RELEASING.md'
        }

        Expand-Archive -Path $zip -DestinationPath $script:Tmp -Force
        $script:Pkg = Join-Path $script:Tmp "twig-$Target"
        if (-not (Test-Path (Join-Path $script:Pkg 'twig.exe'))) { Die 'package is missing twig.exe' }
    }

    # ------------------------------ install pieces ------------------------------

    function Install-Binary {
        $existing = (Get-Command twig -ErrorAction SilentlyContinue).Source
        $old = ''
        if ($existing) { $old = ((& $existing --version) -split ' ')[1] }
        if (-not $script:BinDir) {
            if ($existing -and $existing.EndsWith('.exe')) { $script:BinDir = Split-Path $existing }
            else { $script:BinDir = Join-Path $env:LOCALAPPDATA 'Programs\twig' }
        }
        New-Item -ItemType Directory -Force -Path $script:BinDir | Out-Null
        $exe = Join-Path $script:BinDir 'twig.exe'
        $parked = "$exe.old"
        Remove-Item $parked -Force -ErrorAction SilentlyContinue
        # a running twig.exe (twig update) can't be overwritten, but it can be renamed
        if (Test-Path $exe) { Move-Item $exe $parked -Force }
        Copy-Item (Join-Path $script:Pkg 'twig.exe') $exe
        $script:Twig = $exe
        if ($old) { Ok "twig $old -> $($script:Version.TrimStart('v')) at $exe" }
        else { Ok "twig $($script:Version.TrimStart('v')) installed at $exe" }

        $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        $entries = @($userPath -split ';' | Where-Object { $_ })
        $onPath = $entries | Where-Object { $_.TrimEnd('\') -ieq $script:BinDir.TrimEnd('\') }
        if (-not $onPath) {
            if (Confirm "$($script:BinDir) is not on PATH. Add it to your user PATH?" 'y') {
                [Environment]::SetEnvironmentVariable('Path', (($entries + $script:BinDir) -join ';'), 'User')
                $env:Path = "$env:Path;$($script:BinDir)"
                Ok 'user PATH updated (open a new terminal)'
            } else {
                Warn "add $($script:BinDir) to your PATH"
            }
        }
    }

    function Copy-Skill($dest, $name) {
        New-Item -ItemType Directory -Force -Path (Split-Path $dest) | Out-Null
        if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
        Copy-Item (Join-Path $script:Pkg 'skills\twig') $dest -Recurse
        Ok "$name skill at $dest"
    }

    function Install-Skill {
        $claude = Join-Path $HOME '.claude\skills\twig'
        $agents = Join-Path $HOME '.agents\skills\twig'
        $codex = Join-Path $HOME '.codex\skills\twig'
        if ($Mode -eq 'update') {
            # -Update doesn't ask which agent is used: refresh every copy an earlier install left
            if (Test-Path $claude) { Copy-Skill $claude 'Claude Code' }
            if (Test-Path $agents) { Copy-Skill $agents 'Codex' }
            if (Test-Path $codex) { Copy-Skill $codex 'Codex' }
            return
        }
        if ($script:AI -eq 'codex') { Copy-Skill $agents 'Codex' }
        elseif ($script:AI -or (Test-Path $claude)) { Copy-Skill $claude 'Claude Code' }
    }

    function Install-Herdr-Plugin {
        if (-not (Have herdr)) { Warn 'herdr not found: plugin not installed'; return }
        $pdir = Join-Path $DataDir 'herdr-plugins\twig'
        New-Item -ItemType Directory -Force -Path (Split-Path $pdir) | Out-Null
        if (Test-Path $pdir) { Remove-Item $pdir -Recurse -Force }
        Copy-Item (Join-Path $script:Pkg 'herdr-plugins\twig') $pdir -Recurse
        herdr plugin unlink twig 2>$null | Out-Null
        herdr plugin link $pdir 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) { Warn "could not link the plugin; run: herdr plugin link $pdir"; return }
        Ok "herdr plugin linked ($pdir)"

        $cfg = ''
        if (Test-Path $HerdrConfig) { $cfg = Get-Content $HerdrConfig -Raw }
        if ($cfg -match '# twig:begin') {
            # shortcut already added by a previous install
        } elseif ($cfg -match 'key = "prefix\+shift\+o"') {
            Warn "prefix+shift+o is already bound to another herdr shortcut; point it to ""twig.open-code-windows"" in $HerdrConfig if you want"
        } else {
            New-Item -ItemType Directory -Force -Path (Split-Path $HerdrConfig) | Out-Null
            $block = "`n# twig:begin (managed by the twig installer; ``twig uninstall`` removes it)`n" +
                "[[keys.command]]`nkey = ""prefix+shift+o""`ntype = ""plugin_action""`n" +
                "command = ""twig.open-code-windows""`ndescription = ""open card in VS Code""`n# twig:end`n"
            [IO.File]::AppendAllText($HerdrConfig, $block)
            Ok 'shortcut prefix+shift+o -> twig.open-code-windows'
        }
        herdr server reload-config 2>$null | Out-Null
    }

    function Install-Completions {
        # the hook asks twig itself for the completions, so they follow every update
        $line = 'if (Get-Command twig -ErrorAction SilentlyContinue) { $env:COMPLETE = ''powershell''; twig | Out-String | Invoke-Expression; Remove-Item Env:\COMPLETE }  # twig completions'
        # Windows PowerShell 5.1 and PowerShell 7+ (pwsh) have separate profiles
        $docs = [Environment]::GetFolderPath('MyDocuments')
        $profiles = @(Join-Path $docs 'WindowsPowerShell\Microsoft.PowerShell_profile.ps1')
        if (Have pwsh) { $profiles += Join-Path $docs 'PowerShell\Microsoft.PowerShell_profile.ps1' }
        foreach ($profilePath in $profiles) {
            $text = ''
            if (Test-Path $profilePath) { $text = Get-Content $profilePath -Raw }
            if ($text -match '# twig completions') {
                Ok "completions already enabled in $profilePath"
                continue
            }
            New-Item -ItemType Directory -Force -Path (Split-Path $profilePath) | Out-Null
            [IO.File]::AppendAllText($profilePath, "`n$line`n")
            Ok "completions enabled in $profilePath (open a new terminal)"
        }
        $policy = Get-ExecutionPolicy -Scope CurrentUser
        if ($policy -eq 'Restricted' -or $policy -eq 'Undefined') {
            Info 'if PowerShell refuses to load your profile: Set-ExecutionPolicy -Scope CurrentUser RemoteSigned'
        }
    }

    function Write-Config {
        if (Test-Path $ConfigFile) { Ok "config kept: $ConfigFile"; return }
        Section 'config'
        $repos = Ask 'folder where your repo clones live [~]:' '~'
        $worktrees = Ask 'folder for card worktrees [~/.wt]:' '~/.wt'
        $ai = if ($script:AI) { $script:AI } else { 'claude' }
        $provider = if ($script:Provider) { $script:Provider } else { 'az' }
        New-Item -ItemType Directory -Force -Path (Split-Path $ConfigFile) | Out-Null
        # TOML literal strings ('...'): Windows paths keep their backslashes
        $text = @"
# twig config. Format: https://github.com/$Repo/blob/main/docs/CONFIG.md
version = 1

[agent]
provider = "$ai"

[provider]
default = "$provider"

[provider.az]
organization = '$($script:AzOrg)'

[vscode]
enabled     = $($script:VscodeEnabled)
open_on_new = false

[herdr]
open_on_new = false

[paths]
worktrees = '$worktrees'
repos     = '$repos'

[git]
base_branch   = "main"
branch_prefix = { Bug = "fix", default = "feat" }

# One block per repo (twig config edit):
# [repos.my-backend]
# label   = "Back"
# short   = "back"
# default = true
# run     = "dotnet watch run"
"@
        [IO.File]::WriteAllText($ConfigFile, $text)
        Ok "config created: $ConfigFile"
        Info 'register your repos with `twig config edit` (see docs/CONFIG.md)'
    }

    # ------------------------------ main ------------------------------

    $script:Version = $Version
    $script:BinDir = $BinDir

    Write-Host "twig installer ($Target, $Mode)" -ForegroundColor White
    Write-Host ''
    Warn "herdr's native Windows support is in BETA. twig runs on it, but panes, agent detection"
    Warn 'and the plugin may misbehave. The stable path on Windows is WSL: run install.sh inside'
    Warn 'your WSL distro instead (see the README).'
    if ($Mode -eq 'install' -and -not (Confirm 'Continue with the native Windows install?' 'y')) {
        Info 'cancelled'
        return
    }
    if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
        Warn 'Windows on ARM: installing the x86_64 build (runs under emulation)'
    }

    try {
        if ($Mode -eq 'install') {
            Setup-Git
            Setup-Herdr
            Setup-Vscode
            Setup-Provider
            Setup-AI
        } else {
            if (-not (Have git)) { Warn 'git not found' }
            if (-not (Have herdr)) { Warn 'herdr not found' }
        }

        Download-Release
        Install-Binary
        Install-Skill
        Install-Herdr-Plugin
        Install-Completions

        if ($Mode -eq 'install') {
            Write-Config
            Section 'twig doctor'
            & $script:Twig doctor
        }
        Write-Host ''
        Ok "done: twig $($script:Version.TrimStart('v'))"
    } finally {
        if ($script:Tmp -and (Test-Path $script:Tmp)) { Remove-Item $script:Tmp -Recurse -Force -ErrorAction SilentlyContinue }
    }
}

Install-Twig -Version $Version -Update $Update.IsPresent -BinDir $BinDir -Yes $Yes.IsPresent
