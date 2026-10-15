---
name: twig
description: Per-card work environment (Azure DevOps work item or GitHub issue) with the `twig` CLI — git worktrees of several repos under ~/.wt/<id>/, a herdr workspace with agents, and a VS Code workspace. Use when the user mentions a card/issue number, asks to start, open, list, refresh, or remove a card (e.g. "começa o card 123", "abre o card 123", "terminei o card 123"), asks to add another repo to a card, when the working directory is inside ~/.wt/, or when they talk about worktrees, touching several repos in the same task, or building custom tabs, panes, and workspaces in herdr. Use even if they don't say "twig".
---

# twig: per-card environment

`twig` builds an isolated environment per card. A card usually touches more than one
repository (e.g. back end and front end), so each card gets:

```
~/.wt/<id>/                  card folder (paths.worktrees in the config)
├── <repo-a>/                git worktree of repo A, on the card's branch
├── <repo-b>/                git worktree of repo B
├── CLAUDE.md                card context: work item data, repos, branches, rules
├── <id>.code-workspace      VS Code workspace (only if vscode.enabled)
└── .ado.json | .gh.json     cache of the card data
```

and a **herdr workspace** labeled `<id>`:

- tab `agent`: one pane per repo, running the AI (`ai.command`, e.g. `claude`);
- tab `run`: one pane per repo that has `run` in the config (and `autorun` not `false`).

The repos' main clones stay elsewhere, on the base branch. Never touch them from
a card.

## Before anything else

1. **Inside `~/.wt/<id>/`?** Read `~/.wt/<id>/CLAUDE.md` first. It says what the
   card asks for, which repos and branches are in the card, and has the
   "Contexto do card" section (card context) with notes from previous sessions.
2. **Is `twig` available?** `command -v twig`. If not, look in
   `~/card-cli/target/release/twig`. On environment errors, run `twig doctor`.

## Commands

| Command | When to use |
|---|---|
| `twig new <id> [slug]` | start a new card |
| `twig add-repo <id> <repo,...>` | the card needs another repo; also recreates closed panes in herdr |
| `twig herdr <id>` | open/focus the card in herdr (creates whatever is missing) |
| `twig code [id]` | open the card in VS Code; without an id, the card of the current directory |
| `twig ls` | list active cards and each repo's branch (`*` = uncommitted changes) |
| `twig refresh <id> [--refetch]` | regenerate `CLAUDE.md` and `.code-workspace` (after changing branch or config); `--refetch` fetches the card again |
| `twig rm <id> [--branches]` | close the card (refuses if there are uncommitted changes) |
| `twig prune [-n]` | clean up orphaned worktree records (folder deleted by hand) |
| `twig doctor` | diagnose dependencies, login, config, and repos |
| `twig config edit` | edit the config (interactive, see below) |

`twig new` options:

| Option | Effect |
|---|---|
| `--repos a,b` | the card's repos; without it, uses those with `default = true` |
| `--branch NAME` | forces the branch in all repos |
| `--base NAME` | base for the new branch (default `git.base_branch`) |
| `--fix` | new branch with the `fix/` prefix |
| `--install` | runs each repo's `install` |
| `--herdr` / `--code` | opens herdr / VS Code at the end |
| `--no-ado` | doesn't query the provider |

`twig add-repo` accepts `--branch`, `--base`, `--fix`, `--herdr`, and `--code`.

### How the branch is chosen (per repo)

1. `--branch`, if given.
2. A branch on origin with `<id>` in its name. If there is more than one, `twig` **asks**.
3. None → creates `<prefix>/<id>-<slug>` from `origin/<base>`. The prefix comes from
   `git.branch_prefix` by work item type (e.g. Bug → `fix`), and the slug from the
   card title.

### Running as an agent (no interactive terminal)

`twig` asks in two cases, and without a terminal an empty answer becomes the default:

- **Several branches with the id** → picks the first. If it matters which, pass `--branch`.
- **Repo not in the config and not found** → aborts. Ask the user for the clone's path
  or register the repo in the config (below).

Two pitfalls:

- **`twig herdr <id>` and `--herdr` outside herdr open the herdr UI** and block the
  terminal. If `HERDR_ENV` is not `1`, don't use them; `twig new` already creates the
  herdr workspace on its own. Tell the user to run `twig herdr <id>`.
- **`twig config edit` opens an editor.** As an agent, edit the config file
  directly and validate with `twig doctor`.

## Choosing the right command

- "começa o card 123" (start card 123) → `twig new 123`. If the user named repos, `--repos`.
- "o card 123 também mexe no repo X" (card 123 also touches repo X) → `twig add-repo 123 X`.
- "fechei sem querer o painel do repo X" (accidentally closed repo X's pane) → `twig add-repo 123 X` (recreates what's missing).
- "terminei o card 123" (finished card 123) → check `twig ls` (no `*`), confirm with the user, and only then
  `twig rm 123`. Local branches are only deleted with `--branches`, and only if the user asks.
- "mudei de branch" or "mexi no config" (switched branch / changed the config) → `twig refresh <id>`.
- "dá erro de 'already registered'" ('already registered' error) → `twig prune`.
- Something odd in the environment → `twig doctor` and read the ✖ and ! lines.

## Rules inside a card

- **Work only inside `~/.wt/<id>/`.** Don't edit, check out, or commit in the main
  clones; they are shared by all cards.
- **Don't switch branches inside a worktree.** Another branch is another card.
- **Each repo commits separately.** There is no atomic commit across repos; mention `#<id>`
  in the messages on both sides when the change is coupled.
- **Dependencies and caches are per worktree** (`node_modules`, `bin/`, `obj/`...). Don't
  symlink between cards.
- **The card's `CLAUDE.md` is not versioned.** Update the "Contexto do card" (card context)
  section with decisions and loose ends; it survives `twig refresh`. The rest of the file
  is regenerated.

## Config

Lives at `~/.config/twig/config.toml` (or the path in `TWIG_CONFIG`). Full format
in `docs/CONFIG.md` in the twig repository. The essentials:

```toml
version = 1

[ai]
command = "claude"                # what runs in the agent tab

[provider]
default = "az"                    # "az" | "gh"

[vscode]
enabled     = true
open_on_new = false               # open VS Code at the end of new/add-repo

[herdr]
open_on_new = false               # focus herdr at the end of new/add-repo

[paths]
worktrees = "~/.wt"
repos     = "~"                   # where the main clones live

[git]
base_branch   = "main"
branch_prefix = { Bug = "fix", default = "feat" }

[worktree]
local_files = [".claude/settings.local.json"]   # copied into every repo of the card

[repos.my-backend]
path        = ""                  # empty = <paths.repos>/my-backend
label       = "⚙️ Back"           # name in VS Code
short       = "back"              # pane name in herdr
default     = true                # included when --repos is not passed
run         = "dotnet watch run"  # VS Code task + pane in the run tab
autorun     = true                # false = doesn't start in the run tab
install     = "dotnet restore"    # used by --install
debug       = ["Debug: Back"]     # launch.json configs in the card's compound
local_files = ["src/*/appsettings.Development.json"]  # gitignored, copied from the clone
```

To register a repo: add a `[repos.<name>]` block and run `twig doctor`.
A repo passed in `--repos` that isn't in the config yet is registered automatically if
the clone is found in `paths.repos` or `paths.search`.

## herdr: custom layouts

`twig` only creates the default layout (`agent` + `run`). For anything beyond that
(another tab, an extra pane, a standalone workspace, running and reading a command in
another pane), read [references/herdr.md](references/herdr.md) before running `herdr`
commands.
