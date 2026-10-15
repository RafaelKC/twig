# Dependencies

External tools `twig` uses and how the installer handles each one.

`twig` ships as a native binary (Rust), so the installing machine **doesn't need
Rust**. The tools below are still programs that `twig` calls and must be
installed.

## Summary

| Group | Tool | Rule | Login checked |
|---|---|---|---|
| Required | `git` | always installed | — (checks `user.name` / `user.email`) |
| Required | `herdr` | always installed | — |
| Optional | VS Code (`code`) | asks whether you want it | — |
| Provider (at least one) | `az` (Azure DevOps) | pick one or both | yes |
| Provider (at least one) | `gh` (GitHub) | pick one or both | yes |
| AI (pick one) | Claude Code (`claude`) | only option for now | no (for now) |

## Installer flow

The installer follows the same logic for every dependency:

1. **Detects** whether it's already installed (binary on the `PATH` + version).
2. **Decides** based on the group (required, optional, choice).
3. **Installs** via the OS package manager when needed (`apt` / `brew` / `winget`),
   always showing the command before running it and asking for confirmation when it uses `sudo`.
4. **Configures / logs in** when the tool requires it.

At the end it shows a summary of what was detected, installed, configured and skipped.
All of this is also checked later by `twig doctor`.

### Required: `git` and `herdr`

`twig` doesn't work without them, so there's no question about whether to use them.

- Not installed → installs it.
- Installed → moves on (warns if the version is below the minimum supported).

**git: identity.** After making sure git is present (already there or just installed),
the installer checks the global config:

```bash
git config --global user.name
git config --global user.email
```

- Both set → shows the values and moves on.
- Either empty → asks whether to configure it now. If yes, asks for name and/or email
  and saves them with `git config --global`. If no, moves on and `twig doctor` keeps warning.

### Optional: VS Code (`code`)

Used by `twig code` and by the herdr plugin's `open-code` action.

- Installed → just confirms `code` is on the `PATH`.
- Not installed → asks whether to install it. If not, the commands that open VS Code
  are disabled with a clear message.

> On WSL, `code` comes from Windows VS Code (WSL extension); the installer doesn't install
> VS Code inside the distro, it only gives instructions.

### Project provider: `az` and/or `gh` (at least one)

This is where `twig` gets the work item/issue, title, branch and PR from.

1. Detects which ones are already installed.
2. For each installed one, asks whether to use it.
3. If none was chosen, asks you to pick between `az` and `gh` (at least one) and
   installs the chosen one.
4. For each chosen provider, checks the login and, if not logged in, asks
   whether to log in now:

| Provider | Login check | Login | Extra |
|---|---|---|---|
| `az` | `az account show` | `az login` | `azure-devops` extension (`az extension add --name azure-devops`) and org/project defaults (`az devops configure --defaults organization=… project=…`) |
| `gh` | `gh auth status` | `gh auth login` | — |

If the user declines to log in, the provider stays configured but the commands that
depend on it fail with instructions on how to log in (and `twig doctor` flags it).

### AI: Claude Code (pick one)

Agent that `twig herdr` starts in the `agent` tab.

- For now the list has **only Claude Code**. It's already a choice so that Codex and
  others can be added later without changing the flow.
- Installed → asks whether to use the existing install.
- Not installed → installs it.
- **No login check for now**: login happens on the first run
  of `claude` itself.
- Optional: installs the card skill into `~/.claude/skills/`.

## Not installer dependencies

- `jq`: not needed; `twig` parses JSON internally.
- `dotnet`, `node`/`npm` etc.: needed to **run the card's projects**
  (`run` tab), not for `twig` itself. They're handled by the run strategies and
  `twig doctor`, not the installer.
