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
  each project. Closed a pane by accident? `twig add-repo` brings it back.
- **VS Code workspace.** Friendly folder names, a per-card title bar color, `run:` tasks and a
  compound that debugs every repo at once.
- **Local files.** Gitignored files (`appsettings.Development.json`, `.env.local`, …) are copied
  from the main clone into each worktree.
- **One config.** Everything team-specific lives in `~/.config/twig/config.toml`.
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

Then register your repos and check everything:

```bash
twig config edit
twig doctor
```

| | |
|---|---|
| Update | `twig update` (twig hints on stderr when a new release exists; disable with `TWIG_NO_UPDATE_CHECK=1` or `[update] check = false`) |
| Specific version | `curl -fsSL https://rndm.chicovis.dev/twig/unix-install \| sh -s -- --version v0.2.0` |
| Uninstall | `twig uninstall` |

<details>
<summary>Build from source</summary>

```bash
git clone https://github.com/RafaelKC/twig && cd twig
cargo build --release
ln -s "$PWD/target/release/twig" ~/.local/bin/twig
mkdir -p ~/.config/twig && cp examples/config.toml ~/.config/twig/config.toml
twig completions bash > ~/.local/share/bash-completion/completions/twig
```

</details>

## Usage

```bash
twig new 52644                      # create the card with the default repos
twig new 52644 --repos api,ui-kit   # pick the repos
twig add-repo 52644 web             # add a repo later (or restore closed panes)
twig open 52644                     # open the card in the default target (herdr or code)
twig open-card 52644              # open the card page (Azure DevOps) in the browser
twig herdr 52644                    # open the card in herdr
twig code                           # open the current card in VS Code
twig ls                             # cards, branches, uncommitted changes (*)
twig status 52644                   # ahead/behind, dirty files, herdr agent
twig refresh 52644 --refetch        # regenerate CLAUDE.md / workspace, refetch the card
twig rm 52644 --branches            # done: remove worktrees (and local branches)
```

| Command | What it does |
|---|---|
| `new <id> [slug]` | worktrees, `CLAUDE.md`/`AGENTS.md`, herdr workspace and, if enabled, the VS Code workspace |
| `add-repo <id> <repo,...>` | add repos to a card; recreates missing herdr panes |
| `open [id] [default\|herdr\|code]` | open a card in the chosen target; default from `[open] default` (herdr) |
| `open-card [id]` (`opcd`) | open the card page in the browser (Azure DevOps only for now) |
| `herdr <id>` | open/focus the card in herdr |
| `code [id]` | open the card in VS Code; without an id, the current card |
| `ls` | active cards and each repo's branch (`--json` for scripts) |
| `status [id]` | per repo: branch, uncommitted files, ahead/behind vs base and origin; herdr workspace and agent (`--json` for scripts) |
| `refresh <id>` | regenerate `CLAUDE.md`/`AGENTS.md` and the `.code-workspace` |
| `sync <id> [--merge\|--rebase] [-n]` | fetch the base branch and rebase/merge it into every repo of the card |
| `rm <id> [-f]` | remove the card and its herdr workspace (refuses with uncommitted changes or a running agent; `-f` overrides) |
| `prune` | clean orphaned worktree records |
| `doctor` | check dependencies, logins, config and repos (`--json` for scripts) |
| `config edit` | edit the config and validate it on save |
| `config get <key>` / `config set <key> <value>` | read / write one value by dotted key (`git.base_branch`), validated, comments kept |
| `update` | update to the latest release |
| `uninstall` | remove twig, its plugin and skill |
| `completions <shell>` | shell completion script |

All options: [docs/COMMANDS.md](docs/COMMANDS.md).

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
local_files = ["src/*/appsettings.Development.json"]

[repos.web]
label   = "🌐 Web"
default = true
run     = "pnpm start"
```

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
(per-card ports, more herdr actions, native Windows).

## License

[MIT](LICENSE)
