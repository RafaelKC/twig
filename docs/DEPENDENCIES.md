# Dependencies

External tools `twig` uses and how [`install.sh`](../install.sh) handles each one.

`twig` ships as a native binary, so the machine **doesn't need Rust**. The tools below
are programs `twig` calls and must be installed.

## Summary

| Group | Tool | Rule | Login checked |
|---|---|---|---|
| Required | `git` | installed if missing | — (checks `user.name` / `user.email`) |
| Required | `herdr` | installed if missing | — |
| Optional | VS Code (`code`) | asks; default no | — |
| Provider (at least one) | `az` (Azure DevOps) | use the installed ones, or pick one to install | yes |
| Provider (at least one) | `gh` (GitHub) | same | yes |
| AI (pick one) | Claude Code (`claude`) | only option for now | no |

The installer also needs `curl` and `tar` (present on Linux and macOS).

## How each tool is installed

The installer detects the package manager (`brew`, `apt-get`, `dnf`, `pacman`, `zypper`)
and uses `sudo` when not root.

| Tool | macOS / Homebrew | Debian/Ubuntu | Fedora/RHEL | Arch | Fallback |
|---|---|---|---|---|---|
| `git` | `brew install git` (or Xcode CLT) | `apt-get install git` | `dnf install git` | `pacman -S git` | — |
| `herdr` | `brew install herdr` | `curl -fsSL https://herdr.dev/install.sh \| sh` | same | same | — |
| `gh` | `brew install gh` | official `cli.github.com` apt repo | `dnf install gh` | `pacman -S github-cli` | link to cli.github.com |
| `az` | `brew install azure-cli` | `curl -fsSL https://aka.ms/InstallAzureCLIDeb \| sudo bash` | — | — | link to Microsoft docs |
| VS Code | `brew install --cask visual-studio-code` | `snap install code --classic` | same | same | link; on WSL, install on Windows |
| Claude Code | `curl -fsSL https://claude.ai/install.sh \| bash` | same | same | same | — |

## Installer flow

1. **git**: installs if missing. Then checks `git config --global user.name/user.email`;
   if either is empty, asks to set them now.
2. **herdr**: installs if missing.
3. **VS Code** (optional): found → `vscode.enabled = true`; missing → asks to install
   (default no). On WSL, `code` comes from Windows VS Code, so it only gives instructions.
4. **Provider**: for each of `az`/`gh` already installed, asks whether to use it. None
   chosen → asks which one to install. For each chosen provider:

   | Provider | Checks | If it fails |
   |---|---|---|
   | `az` | `azure-devops` extension; default organization (asks for it if missing); access with `az devops project list` | adds the extension; asks to run `az login` |
   | `gh` | `gh auth status` | asks to run `gh auth login` |

   `az account show` is not used on purpose: access through `az devops login` (PAT) works
   without it.
5. **AI**: Claude Code installed → asks whether to use it; missing → asks to install.
   No login check: `claude` asks on first run.
6. **twig**: downloads the release for the OS/arch, verifies the checksum and, when
   `cosign` is installed, the Sigstore signature (see [RELEASING.md](RELEASING.md)).
   Installs the binary, the AI skill (`~/.claude/skills/twig`), the herdr plugin (linked,
   plus the `prefix+shift+o` shortcut unless that key is taken) and shell completions.
7. **Config**: only if `~/.config/twig/config.toml` doesn't exist. Asks for the repos folder
   and the worktrees folder and writes the provider, organization and `vscode.enabled`
   chosen above. Repos are not registered here: `twig config edit`.
8. Runs `twig doctor`.

Without a terminal (CI, `--yes`), every question takes its default: required tools are
installed, optional ones are skipped, installed providers are used.

**Running it again updates**: dependencies are only installed if missing, the config is
kept, and twig, the skill, the plugin and completions are replaced. `twig update` runs the
installer in `--update` mode, which skips the dependency steps.

## Not installer dependencies

- `jq`: not needed; `twig` parses JSON internally.
- `dotnet`, `node`/`npm` etc.: needed to **run the card's projects** (`run` tab), not
  for `twig` itself.
