# Roadmap

What's done and what's left for `twig` to be a tool any team can install.

Issue numbers refer to <https://github.com/RafaelKC/twig/issues>.

## Releases

### Next: v0.3.0

Not planned yet; pick from [Later](#later).

### Later

Not scheduled yet; grouped by theme.

- **Ports and run:** [#3](https://github.com/RafaelKC/twig/issues/3) per-card dev server ports (card A front on 4200, card B on 4201)
- **GitHub provider:** [#4](https://github.com/RafaelKC/twig/issues/4) `owner/repo#123` ids (unblocks `open-card` for `gh`)
- **Automation:** [#5](https://github.com/RafaelKC/twig/issues/5) run a skill after creating a workspace; [#15](https://github.com/RafaelKC/twig/issues/15) lifecycle hooks; [#14](https://github.com/RafaelKC/twig/issues/14) card templates
- **Day-to-day commands:** [#9](https://github.com/RafaelKC/twig/issues/9) `status`; [#10](https://github.com/RafaelKC/twig/issues/10) `clean` (merged/closed cards); [#12](https://github.com/RafaelKC/twig/issues/12) `pr`; [#13](https://github.com/RafaelKC/twig/issues/13) `sync`; [#11](https://github.com/RafaelKC/twig/issues/11) `--json`
- **Agents and providers:** [#8](https://github.com/RafaelKC/twig/issues/8) more agents; [#16](https://github.com/RafaelKC/twig/issues/16) handoff notes; [#17](https://github.com/RafaelKC/twig/issues/17) Jira/Linear/GitLab
- **Config and quality:** [#19](https://github.com/RafaelKC/twig/issues/19) `config get/set`, `.twig.toml`; [#18](https://github.com/RafaelKC/twig/issues/18) CI on PRs and integration tests

Current: v0.2.0 (`rm` closes the herdr workspace and blocks on a running agent, `open`, `open-card`, Codex support; on top of v0.1.1's installer, `update`, `uninstall`, `doctor`, `prune`, completions, `worktree.auto_install` and per-repo `post_create`).

## Product and name

- [ ] Pick a final name for the CLI (`twig` is provisional)
- [x] Generic core: everything team-specific (repos, run/install commands, local files, provider, branch prefixes) lives in the config
- [x] herdr plugin and AI skill named after the CLI (`herdr-plugins/twig`, `skills/twig`)

## Work item / repository provider

- [x] Support the `az` CLI (Azure DevOps) **and** the `gh` CLI (GitHub)
  - [x] Single provider interface: title, type, state, assignee, tags, description, acceptance criteria and link, cached per card
  - [ ] The branch's PR (status, link) through the provider ([#12](https://github.com/RafaelKC/twig/issues/12))
  - [ ] GitHub `owner/repo#123` ids (today the issue is looked up in the first repo of the card) ([#4](https://github.com/RafaelKC/twig/issues/4))
- [ ] More providers: Jira, Linear, GitLab ([#17](https://github.com/RafaelKC/twig/issues/17))
- [x] At install, ask which CLI to use (az and/or gh)
- [x] If the chosen CLI isn't installed, install it (brew/apt/dnf/pacman)
- [x] If not logged in, run the login (`az login` / `gh auth login`) and set the default ADO organization
- [ ] Offline mode (no provider) chosen at install

## Install and update

- [x] Prebuilt, signed binaries for Linux and macOS on GitHub Releases on every `v*` tag ([RELEASING.md](RELEASING.md))
- [ ] Windows binaries and a PowerShell installer (see [Native Windows](#native-windows))
- [x] Installer (`curl ... | sh`) that:
  - [x] Puts `twig` on the PATH and installs shell completions
  - [x] Checks and installs dependencies (see [DEPENDENCIES.md](DEPENDENCIES.md))
  - [x] Asks for the folder where repos live and the worktrees folder (default `~/.wt`)
  - [x] Installs the AI skill and the herdr plugin (with the `prefix+shift+o` shortcut)
  - [ ] Installs the third-party plugins approved in `herdr-plugins/README.md` (pinned refs) and writes the shortcuts to herdr's `config.toml` without duplicating them
- [x] `twig update`: finds a newer release and reinstalls twig, plugin and skill
  - [x] Update hint after commands: cached check, stderr only, `[update]` config / `TWIG_NO_UPDATE_CHECK` ([#20](https://github.com/RafaelKC/twig/issues/20))
  - [ ] Migrate the config between schema versions and show the changelog
- [x] `twig uninstall`: removes twig, plugin, skill and completions; asks about config and az/gh/herdr
- [x] `twig doctor`: checks dependencies, login, config and repos

## Config

- [x] Config file (`~/.config/twig/config.toml`, or `TWIG_CONFIG`)
  - [x] Folders, repos, labels, provider, base branch, branch prefixes, run/install commands, local files, debug configs
  - [x] Repos passed with `--repos` that aren't in the config get registered automatically
  - [ ] Per-repo override (versioned `.twig.toml` in the repo) and per-card override ([#19](https://github.com/RafaelKC/twig/issues/19))
  - [ ] Environment variable overrides and their precedence ([#19](https://github.com/RafaelKC/twig/issues/19))
- [x] `twig config edit`
- [ ] `twig config get/set` ([#19](https://github.com/RafaelKC/twig/issues/19))
- [x] `[open] default` target for `twig open` ([#1](https://github.com/RafaelKC/twig/issues/1))
- [ ] Card templates ([#14](https://github.com/RafaelKC/twig/issues/14)) and lifecycle hooks ([#15](https://github.com/RafaelKC/twig/issues/15))
- [ ] Agent choice: Codex ([#2](https://github.com/RafaelKC/twig/issues/2)), more agents ([#8](https://github.com/RafaelKC/twig/issues/8))

## Run and ports per project type

- [ ] Strategy that identifies the project type and knows how to run it and where to set the port
  - [ ] .NET (`*.csproj` / `launchSettings.json` / `ASPNETCORE_URLS`)
  - [ ] Angular (`angular.json` / `ng serve --port` / `environment.ts`)
  - [ ] Generic Node (`package.json` scripts, `PORT`)
  - [ ] Others: Vite, Next, Python, Go…
- [ ] Per-card ports, so two cards can run at the same time ([#3](https://github.com/RafaelKC/twig/issues/3))
- [ ] Users can write their own strategy (script/plugin in the config folder, with a contract: `detect`, `run_cmd`, `apply_port`, `default_port_base`)
- [ ] Wire front ↔ back automatically (front pointing at the same card's back-end port)

## Commands (ideas, see issues)

- [x] `twig rm` also removes the herdr workspace, blocks on a running agent, `-f` ([#6](https://github.com/RafaelKC/twig/issues/6))
- [x] `twig open [id] [default|herdr|code]` ([#1](https://github.com/RafaelKC/twig/issues/1))
- [x] `twig open-card` / `opcd` ([#7](https://github.com/RafaelKC/twig/issues/7))
- [x] `twig status` ([#9](https://github.com/RafaelKC/twig/issues/9); ports and PR info pending #3/#12)
- [ ] `--json` ([#11](https://github.com/RafaelKC/twig/issues/11))
- [ ] `twig clean` ([#10](https://github.com/RafaelKC/twig/issues/10)), `twig pr` ([#12](https://github.com/RafaelKC/twig/issues/12))
- [x] `twig sync` ([#13](https://github.com/RafaelKC/twig/issues/13))
- [ ] Run a skill after creating a workspace ([#5](https://github.com/RafaelKC/twig/issues/5)), handoff notes ([#16](https://github.com/RafaelKC/twig/issues/16))

## herdr integration (plugin)

- [x] "Open card in VS Code" action (`prefix+shift+o`)
- [ ] "Open card in ADO/GitHub" action (browser)
- [ ] "Open card's PR" action (or warn if it doesn't exist)
- [ ] "Open running front end" action (`localhost:<card port>`)
- [ ] "Close card" action (after [#6](https://github.com/RafaelKC/twig/issues/6) `rm` also closes the workspace; the action itself is still missing): `twig rm` + `herdr workspace close` (manual today)
- [ ] Link handler: Ctrl+click on a card/PR URL → `twig herdr <id>`
- [ ] Popup picker (fzf) of cards in `~/.wt` + cards assigned to me → opens/creates the workspace
- [ ] Card status in the sidebar via `workspace report-metadata` (card state, PR status/build, app running) with caching
- [ ] Notification when a card's agent goes `blocked` waiting for input
- [ ] "Restart app" action in the `run` tab

## PR review from the terminal (herdr-reviewr)

Use [herdr-reviewr](https://github.com/persiyanov/herdr-reviewr) to review PRs without leaving herdr.
**Not installed** yet; see `refs/herdr-plugins.md`.

- [ ] Install reviewr (review the code, pin the commit) with `auto_open = false`, because it opens on its own in worktree workspaces and would mess up the `agent`/`run` layout of `twig herdr`
- [ ] Shortcut to open/close (`persiyanov.reviewr.toggle`)
- [ ] Flow for reviewing someone else's PR:
  1. `twig new <id> --branch <PR-branch> --herdr` → isolated checkout in a worktree
  2. reviewr in `branch` scope (`b`) = PR diff against the `main` merge-base; PR tab (`3`) reads the PR via `az`/`gh` (read-only)
  3. comment per line/block, and `s` sends the comments to the pane's agent
  4. the agent posts each comment **inline on the PR** and approves/requests changes if asked
- [ ] Review-PR skill: takes comments in reviewr's format (file + lines + text), finds the branch's PR and posts them as inline threads on ADO or GitHub
- [ ] Action in `twig`/plugin: "review PR <n>" that creates the worktree from the PR (not the card) and opens reviewr
- [ ] Use the same reviewr to review the agent's own diff before opening the PR (`uncommitted` / `last turn` scopes)

## VS Code

- [x] Don't leak `HERDR_*` into VS Code on `twig code`
- [ ] Generate `terminal.integrated.env.linux` in the `.code-workspace` clearing `HERDR_*`
- [ ] Option to open the card's herdr in a terminal in the editor area

## Native Windows

Windows works today through WSL (Linux binary and `install.sh`). Native Windows depends on
herdr's native Windows support, which is still in beta, so this waits until it's stable.

- [ ] Build `x86_64-pc-windows-msvc` in the release workflow and publish a `.zip` (signed checksums like the other targets)
- [ ] `install.ps1` (`irm …/install.ps1 | iex`), mirroring `install.sh`:
  - [ ] Dependencies via winget: `Git.Git`, `GitHub.cli`, `Microsoft.AzureCLI`, `Microsoft.VisualStudioCode`
  - [ ] herdr via its installer (`irm https://herdr.dev/install.ps1 | iex`) and Claude Code via its Windows installer
  - [ ] Binary in `%LOCALAPPDATA%\Programs\twig`, added to the user `PATH`; skill, herdr plugin and PowerShell completions
- [ ] `twig update` runs `install.ps1` on Windows
- [ ] `twig uninstall` removes dependencies with `winget uninstall`
- [ ] herdr plugin: Windows variant of `open-code` (`.cmd`/`.ps1`) and `windows` in `platforms`
- [ ] `.code-workspace`: also set `terminal.integrated.env.windows`
- [ ] Review Unix-only assumptions (paths, `sh -c` in `run`/`install` commands, process groups when starting the herdr server)
- [ ] Integration tests on a Windows runner in CI
- [ ] Optional: Authenticode signing, so SmartScreen doesn't warn on first run

## Quality

- [x] Integration tests for `doctor`, `config edit`, `code` and the herdr plugin (fake tools on a controlled `PATH`)
- [x] Integration tests for `new` / `add-repo` / `rm` / `prune` against local git remotes ([#18](https://github.com/RafaelKC/twig/issues/18))
- [x] `scripts/release.sh` opens a `release/vX.Y.Z` PR and tags after merge (`--tag`), so it works with branch rules on `main` ([#22](https://github.com/RafaelKC/twig/issues/22); enabling the ruleset itself is still manual, see [RELEASING.md](RELEASING.md))
- [ ] CI on pull requests: `cargo fmt --check`, `cargo clippy`, `cargo test`, `shellcheck install.sh` (today tests only run on release tags) ([#18](https://github.com/RafaelKC/twig/issues/18))
- [x] Translate runtime messages and the generated `CLAUDE.md` to English (or make them localizable)
- [ ] Test on macOS (today used on Linux/WSL)
