# Roadmap

What's done and what's left for `twig` to be a tool any team can install.

## Product and name

- [ ] Pick a final name for the CLI (`twig` is provisional)
- [x] Generic core: everything team-specific (repos, run/install commands, local files, provider, branch prefixes) lives in the config
- [x] herdr plugin and AI skill named after the CLI (`herdr-plugins/twig`, `skills/twig`)

## Work item / repository provider

- [x] Support the `az` CLI (Azure DevOps) **and** the `gh` CLI (GitHub)
  - [x] Single provider interface: title, type, state, assignee, tags, description, acceptance criteria and link, cached per card
  - [ ] The branch's PR (status, link) through the provider
  - [ ] GitHub `owner/repo#123` ids (today the issue is looked up in the first repo of the card)
- [ ] At install, ask which CLI to use (az, gh, or none = offline mode)
- [ ] If the chosen CLI isn't installed, install it (apt/brew/winget depending on OS)
- [ ] If not logged in, run the login (`az login` / `gh auth login`) and validate the default org/project

## Install and update

- [ ] Prebuilt binaries for Linux, macOS and Windows on GitHub Releases (e.g. `cargo-dist`)
- [ ] Installer (`curl ... | sh` / PowerShell) that:
  - [ ] Puts `twig` on the PATH and installs shell completions
  - [ ] Checks and installs dependencies (see [DEPENDENCIES.md](DEPENDENCIES.md))
  - [ ] Asks for the folder where repos live and the worktrees folder (default `~/.wt`)
  - [ ] Optional: installs the AI skill and the herdr plugin
  - [ ] Installs the third-party plugins approved in `herdr-plugins/README.md` (pinned refs) and writes the shortcuts to herdr's `config.toml` without duplicating them
- [ ] Updater (`twig update`): downloads the new version, migrates config, shows the changelog
- [x] `twig doctor`: checks dependencies, login, config and repos

## Config

- [x] Config file (`~/.config/twig/config.toml`, or `TWIG_CONFIG`)
  - [x] Folders, repos, labels, provider, base branch, branch prefixes, run/install commands, local files, debug configs
  - [x] Repos passed with `--repos` that aren't in the config get registered automatically
  - [ ] Per-repo override (versioned `.twig.toml` in the repo) and per-card override
  - [ ] Environment variable overrides and their precedence
- [x] `twig config edit`
- [ ] `twig config get/set`

## Run and ports per project type

- [ ] Strategy that identifies the project type and knows how to run it and where to set the port
  - [ ] .NET (`*.csproj` / `launchSettings.json` / `ASPNETCORE_URLS`)
  - [ ] Angular (`angular.json` / `ng serve --port` / `environment.ts`)
  - [ ] Generic Node (`package.json` scripts, `PORT`)
  - [ ] Others: Vite, Next, Python, Go…
- [ ] Per-card ports, so two cards can run at the same time
- [ ] Users can write their own strategy (script/plugin in the config folder, with a contract: `detect`, `run_cmd`, `apply_port`, `default_port_base`)
- [ ] Wire front ↔ back automatically (front pointing at the same card's back-end port)

## herdr integration (plugin)

- [x] "Open card in VS Code" action (`prefix+shift+o`)
- [ ] "Open card in ADO/GitHub" action (browser)
- [ ] "Open card's PR" action (or warn if it doesn't exist)
- [ ] "Open running front end" action (`localhost:<card port>`)
- [ ] "Close card" action: `twig rm` + `herdr workspace close` (manual today)
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

## Quality

- [x] Integration tests for `doctor`, `config edit`, `code` and the herdr plugin (fake tools on a controlled `PATH`)
- [ ] Integration tests for `new` / `add-repo` / `rm` / `prune` against local git remotes
- [ ] CI: `cargo fmt --check`, `cargo clippy`, `cargo test` on Linux, macOS and Windows
- [ ] Translate runtime messages and the generated `CLAUDE.md` to English (or make them localizable)
- [ ] macOS and native Windows support (today used on Linux/WSL)
