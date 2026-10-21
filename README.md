<div align="center">

🇺🇸 English | [🇧🇷 Português](README.pt-BR.md)

# 🌿 twig

**One isolated workspace per card.**
Git worktrees across all the repos a task touches, an AI agent per repo in
[herdr](https://herdr.dev), and a color-coded VS Code workspace, all from one command.

![Rust](https://img.shields.io/badge/rust-2021-orange?logo=rust)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)
![Status](https://img.shields.io/badge/status-early-yellow)

</div>

---

```console
$ twig new 52644
→ card: Vehicle category per route
→ [api] fetch...
✔ [api] feat/52644-vehicle-category-per-route
→ [web] fetch...
✔ [web] feat/52644-vehicle-category-per-route
✔ VS Code workspace generated (color #af601a, compound with 2 processes)
✔ CLAUDE.md generated in ~/.wt/52644
✔ herdr workspace #52644 created
```

## Why

A real task rarely lives in one repo. The back end, the front end and a shared
library all change together, and switching branches in the main clones to juggle
two tasks breaks everything that's running. `twig` gives every card (an Azure
DevOps work item or a GitHub issue) its own sandbox:

```
~/.wt/52644/
├── api/                    git worktree on the card's branch
├── web/                    git worktree on the card's branch
├── CLAUDE.md               (AGENTS.md for Codex) the card's title, description, acceptance criteria, repos and rules
├── 52644.code-workspace    VS Code workspace, colored per card, with run tasks and a debug compound
├── MEMORY.md               card memory (symlink; survives `twig rm`)
├── .twig-ports.env         the card's dev server ports (4244, 5044: same for this card every time)
└── .ado.json               cached work item
```

Your main clones stay on `main`. Two cards never step on each other.

## Features

- **Worktrees across repos.** One `twig new <id>` creates a worktree per repo. The branch is
  picked for you: an existing `origin` branch with the id in its name, or a new
  `feat/<id>-<slug>` (or `fix/` for bugs), with the slug taken from the card's title.
- **Card context for the agent.** Title, state, description and acceptance criteria are pulled
  from Azure DevOps (`az`) or GitHub (`gh`) into the card's `CLAUDE.md` (or `AGENTS.md` with Codex), along with a
  "card context" section that survives regeneration.
- **herdr workspace.** An `agent` tab with one AI agent per repo, and a `run` tab that starts
  each project. Closed a pane by accident? `twig add-repo` brings it back. `twig new --prompt
  "/deliver {id}"` (or `agent_prompt` in a template) starts the agents already working.
- **Per-card ports.** A repo with `port = 4200` (and named extras like `ports = { https = 4300 }`) runs on ports of its own in each card,
  deterministic per card (card 52644 → `4244`), so several cards run at once. Every process of
  the card gets every repo's port (`TWIG_PORT_<REPO>`, `PORT`), and the repo's `env` maps them to
  what the project reads: the front end calls the same card's API, the API allows that front end
  in CORS.
- **Pull requests and cleanup.** `twig pr` pushes and opens one PR per repo, linked to the card
  and to each other; `twig clean` removes the cards whose branches were merged or whose work item
  was closed.
- **GitHub or Azure DevOps.** `twig new 52644` or `twig new acme/web#12`; both providers sit
  behind one interface.
- **VS Code workspace.** Friendly folder names, a per-card title bar color, `run:` tasks and a
  compound that debugs every repo at once.
- **Local files.** Gitignored files (`appsettings.Development.json`, `.env.local`, …) are copied
  from the main clone into each worktree.
- **Lifecycle hooks.** Your own shell commands on `post_create`, `pre_rm`, `post_rm`, `post_open` and `post_refresh`, card-wide or per repo; `--no-hooks` skips them.
- **Card templates.** Named presets in `~/.config/twig/templates.toml` (repos, agent, install, hooks): `twig new 123 --template fullstack`; flags override them.
- **One config, plus the repo's.** Your settings live in `~/.config/twig/config.toml`; a repo
  can version its own (`run`, `install`, `port`, `env`) in a `.twig.toml`, a card can override
  them in `<card>/.twig.toml`, and `TWIG__SECTION__KEY` env vars override anything.
- **Self-check.** `twig doctor` checks tools, logins, config and repos, and finds orphaned worktrees.

## herdr layout

```
┌─ workspace "52644" ─────────────────────────────────────────┐
│ [agent]  run                                                │
│ ┌─────────────────────────┬─────────────────────────┐       │
│ │ api                     │ web                     │       │
│ │ $ claude                │ $ claude                │       │
│ │                         │                         │       │
│ └─────────────────────────┴─────────────────────────┘       │
└─────────────────────────────────────────────────────────────┘
```

## Requirements

The installer takes care of these.

| Tool | |
|---|---|
| `git` | required |
| [`herdr`](https://herdr.dev) | required |
| `az` (+ `azure-devops` extension) **or** `gh` | at least one, to read cards |
| `claude` ([Claude Code](https://claude.com/claude-code)) | the AI agent started per repo |
| `code` (VS Code) | optional |

Details in [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md).

## Install

Linux (x86_64, arm64, WSL) and macOS (Intel, Apple Silicon):

```bash
curl -fsSL https://rndm.chicovis.dev/twig/unix-install | sh
```

The installer checks and installs the dependencies (git, herdr, az/gh, Claude Code,
optionally VS Code), sets up logins, downloads the release for your system, verifies its
checksum and [Sigstore signature](docs/RELEASING.md#signing-and-verification), and
installs twig, the AI skill, the herdr plugin and shell completions. Running it again
updates. Details: [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md).

### Windows

Two options:

- **WSL (recommended, stable):** open your WSL distro and run the Linux command above. twig,
  herdr and the agents run inside WSL; VS Code on Windows opens the cards through its WSL
  extension.
- **Native Windows (beta):** in PowerShell (5.1 or 7):

  ```powershell
  irm https://github.com/RafaelKC/twig/releases/latest/download/install.ps1 | iex
  ```

  > **herdr's native Windows support is in beta.** twig works on it, but panes, agent
  > detection and the herdr plugin may misbehave. The installer warns about it before
  > changing anything, and `twig doctor` keeps reminding you.

  It installs the missing dependencies with winget (Git, GitHub CLI or Azure CLI, VS Code)
  and the official installers (herdr, Claude Code), puts `twig.exe` in
  `%LOCALAPPDATA%\Programs\twig` (added to your user PATH), the skill, the herdr plugin
  (`prefix+shift+o`) and the completions in your PowerShell profile. The config lives in
  `%APPDATA%\twig\config.toml`. Options go through environment variables with `irm | iex`
  (`$env:TWIG_VERSION = "v0.4.0"`), or as parameters when you save the script
  (`.\install.ps1 -Version v0.4.0`). `twig update` and `twig uninstall` work the same as on
  Linux.

  On Windows, `install`/`post_create` and hooks run through `cmd /C` and `run` in the herdr
  pane's shell (PowerShell), so prefer the repo's [`env`](docs/CONFIG.md#repo-environment-env)
  (expanded by twig on every OS) over `${PORT:-4200}`, which is `sh` syntax.

Then register your repos and check everything:

```bash
twig config edit
twig doctor
```

| | |
|---|---|
| Update | `twig update` (twig hints on stderr when a new release exists; disable with `TWIG_NO_UPDATE_CHECK=1` or `[update] check = false`) |
| Specific version | `curl -fsSL https://rndm.chicovis.dev/twig/unix-install \| sh -s -- --version v0.2.0`; Windows: `$env:TWIG_VERSION = "v0.4.0"` before `irm \| iex` |
| Uninstall | `twig uninstall` |

<details>
<summary>Build from source</summary>

```bash
git clone https://github.com/RafaelKC/twig && cd twig
cargo build --release
ln -s "$PWD/target/release/twig" ~/.local/bin/twig
mkdir -p ~/.config/twig && cp examples/config.toml ~/.config/twig/config.toml
echo 'source <(COMPLETE=bash twig)  # twig completions' >> ~/.bashrc
```

</details>

## Usage

```bash
twig new 52644                      # create the card with the default repos
twig new 52644 --repos api,ui-kit   # pick the repos
twig add-repo 52644 web             # add a repo later (or restore closed panes)
twig open 52644                     # open the card in the default target (herdr or code)
twig open-card 52644              # open the card page in the browser
twig new acme/web#12               # GitHub: issue 12 of acme/web (card folder web-12)
twig herdr 52644                    # open the card in herdr
twig code                           # open the current card in VS Code
twig ls                             # cards, branches, uncommitted changes (*)
twig status 52644                   # ahead/behind, dirty files, herdr agent
twig refresh 52644 --refetch        # regenerate CLAUDE.md / workspace, refetch the card
twig memo "validation moved to API" # (in development) note for the next agent session
twig rm 52644 --branches            # done: remove worktrees (and local branches)
```

| Command | What it does |
|---|---|
| `new <id> [slug]` | worktrees, `CLAUDE.md`/`AGENTS.md`, herdr workspace and, if enabled, the VS Code workspace |
| `add-repo <id> <repo,...>` | add repos to a card; recreates missing herdr panes |
| `open [id] [default\|herdr\|code]` | open a card in the chosen target; default from `[open] default` (herdr) |
| `open-card [id]` (`opcd`) | open the card page (work item or issue) in the browser |
| `herdr <id>` | open/focus the card in herdr |
| `code [id]` | open the card in VS Code; without an id, the current card |
| `ls` | active cards and each repo's branch (`--json` for scripts) |
| `status [id]` | per repo: branch, uncommitted files, ahead/behind vs base and origin; herdr workspace and agent (`--json` for scripts) |
| `refresh <id>` | regenerate `CLAUDE.md`/`AGENTS.md` and the `.code-workspace` |
| `memo [text] [-c id]` | (in development) append a dated line to the card memory, kept across sessions and `rm`; no text shows it |
| `sync <id> [--merge\|--rebase] [-n]` | fetch the base branch and rebase/merge it into every repo of the card |
| `clean [-n] [-y]` | remove finished cards (branches merged or work item closed), asking per card |
| `ports [id] [--env]` | show the card's dev server ports (deterministic per card, given to the panes as `TWIG_PORT_<REPO>` / `PORT`) |
| `pr [id] [--draft] [-n]` | push each repo's branch and open one PR per repo, linked to the card and to each other |
| `rm <id> [-f]` | remove the card and its herdr workspace (refuses with uncommitted changes or a running agent; `-f` overrides) |
| `prune` | clean orphaned worktree records |
| `doctor` | check dependencies, logins, config and repos (`--json` for scripts) |
| `template ls` / `show NAME` / `edit` | list, show and edit the card templates (`new --template NAME` applies one) |
| `config edit` | edit the config and validate it on save |
| `config get <key>` / `config set <key> <value>` | read / write one value by dotted key (`git.base_branch`), validated, comments kept |
| `update` | update to the latest release |
| `uninstall` | remove twig, its plugin and skill |
| `completions <shell>` | shell completion hook (the installer adds it to `~/.bashrc` / `~/.zshrc`); completes card ids, repos and templates too |

All options: [docs/COMMANDS.md](docs/COMMANDS.md).

### Card memory (in development)

Agents lose context between sessions. `twig memo "..."` appends one dated line to a per-card
`MEMORY.md`. The real file lives in `<worktrees>/memos/<id>-<provider>/` and the card folder
holds a relative symlink to it, so the notes **survive `twig rm`** and come back when
`twig new <id>` reopens the card. The last 10 entries are copied into `CLAUDE.md`/`AGENTS.md`
on `new`, `add-repo` and `refresh`. This is the first, simple version of
[#16](https://github.com/RafaelKC/twig/issues/16) and the format may still change; design and
what is missing (consolidation, size cap, export on `rm`) in [docs/MEMORY.md](docs/MEMORY.md).

## Configuration

```toml
version = 1

[provider]
default = "az"                      # or "gh"

[paths]
worktrees = "~/.wt"
repos     = "~/dev"

[repos.api]
label   = "⚙️ API"                  # VS Code folder name
short   = "api"                     # herdr pane name
default = true                      # part of every new card
run     = "dotnet watch run --project src/Api"
install = "dotnet restore"
port    = 5000                      # 5000 + the card's offset in each card
env     = { ASPNETCORE_URLS = "http://localhost:$PORT", Cors__Origins__0 = "http://localhost:${TWIG_PORT_WEB:-4200}" }
local_files = ["src/*/appsettings.Development.json"]

[repos.web]
label   = "🌐 Web"
default = true
run     = "ng serve --port ${PORT:-4200} --proxy-config proxy.conf.js"   # proxy reads $TWIG_PORT_API
port    = 4200
```

The same repo settings can live in the repo itself, versioned, as a `.twig.toml`
([example](examples/repo.twig.toml)).

Full example: [examples/config.toml](examples/config.toml). Every field:
[docs/CONFIG.md](docs/CONFIG.md).

## Extras

| | |
|---|---|
| [herdr-plugins/twig](herdr-plugins/README.md) | herdr plugin: `prefix+shift+o` opens the focused card in VS Code |
| [skills/twig](skills/twig/SKILL.md) | AI agent skill: how to use twig, plus building custom herdr tabs, panes and workspaces |

## Project layout

```
src/              the twig CLI (Rust)
tests/            integration tests (fake tools on a controlled PATH)
examples/         example config
herdr-plugins/    herdr plugin
skills/           AI agent skill
scripts/          release helper
.github/          release workflow
install.sh        installer
docs/             commands, config, dependencies, releasing, roadmap
refs/             evaluated but unused references
```

```bash
cargo test        # unit + integration tests
```

## Status

Early and opinionated: built for a daily Azure DevOps + .NET + Angular workflow on
Linux/WSL, now generalized through the config. See the [roadmap](docs/ROADMAP.md) for what's next
(more herdr actions, native Windows out of beta along with herdr's).

## License

[MIT](LICENSE)
