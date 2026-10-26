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
│   └── .twig.toml           (optional, versioned in the repo) the repo's twig settings
├── <repo-b>/                git worktree of repo B
├── CLAUDE.md                card context: work item data, repos, branches, ports, rules
│                            (AGENTS.md when agent.provider = "codex")
├── MEMORY.md                card memory (symlink into <worktrees>/memos/; survives rm)
├── <id>.code-workspace      VS Code workspace (only if vscode.enabled)
├── .twig-ports.env          the card's dev server ports (only if some repo has `port`)
├── .twig.toml               (optional, by hand) repo settings for this card only
├── .twig-template           template the card was created from
├── .twig-issue              GitHub owner/repo#N of a repo-scoped card
└── .ado.json | .gh.json     cache of the card data
```

`<id>` is the card number (`52644`), or `<repo>-<number>` for a GitHub card created from
`owner/repo#123` (folder `repo-123`); afterwards both forms name the card.

and a **herdr workspace** labeled `<id>`:

- tab `agent`: one pane per repo, running the agent (`agent.command`, e.g. `claude` or `codex`);
- tab `run`: one pane per repo that has `run` in the config (and `autorun` not `false`; a template's `autorun` overrides it).

Every pane twig creates gets the card's ports (`TWIG_PORT_<REPO>` for each repo,
`TWIG_PORT_<REPO>_<NAME>` for named ports, `PORT` for its own main one) and the repo's `env` as
environment variables.

The repos' main clones stay elsewhere, on the base branch. Never touch them from
a card.

On **native Windows** (beta, like herdr's Windows support) the card folder is
`%USERPROFILE%\.wt\<id>\` by default, the config is `%APPDATA%\twig\config.toml`, herdr panes run
PowerShell, and `install`/hooks run through `cmd /C`: write commands for that shell, and use
`$env:PORT` / the repo's `env` instead of `${PORT:-4200}`. On WSL everything is as on Linux.

## Before anything else

1. **Inside `~/.wt/<id>/`?** Read `~/.wt/<id>/CLAUDE.md` (or `AGENTS.md` for Codex) first. It says what the
   card asks for, which repos and branches are in the card, and has the
   "Card context" section with notes from previous sessions.
2. **Is `twig` available?** `command -v twig`. If not, look in
   `~/card-cli/target/release/twig`. On environment errors, run `twig doctor`.

## Commands

| Command | When to use |
|---|---|
| `twig new <id> [slug] [--prompt TEXT]` | start a new card. With GitHub, prefer `owner/repo#123` (or the issue URL): the card folder becomes `repo-123`, and either form names it afterwards |
| `twig add-repo <id> <repo,...>` | the card needs another repo; also recreates closed panes in herdr |
| `twig open [id] [default\|herdr\|code]` | open the card in a target; no target uses `[open] default` (herdr unless configured); no id = card of the current directory. `twig open code` and `twig open 123 code` both work |
| `twig herdr <id>` | alias of `open <id> herdr`: open/focus the card in herdr (creates whatever is missing) |
| `twig code [id]` | alias of `open [id] code`: open the card in VS Code |
| `twig open-card [id]` (alias `opcd`) | open the card's page (work item or issue) in the browser; prints the URL if no opener exists |
| `twig ls [--json]` | list active cards and each repo's branch (`*` = uncommitted changes) |
| `twig status [id] [--json]` | per repo: branch, uncommitted files (`*N`), ahead/behind vs base and origin; herdr workspace and running agent. No id = all cards, compact |
| `twig refresh <id> [--refetch]` | regenerate the context file (`CLAUDE.md`/`AGENTS.md`) and `.code-workspace` (after changing branch or config); `--refetch` fetches the card again |
| `twig clean [-n] [-y]` | remove finished cards (every branch merged, or the work item closed); `-n` lists them. Never pass `--yes`/`--force` unless the user asked for it |
| (config) | repo settings can come from a `.twig.toml` versioned in the repo, a card-only `<card>/.twig.toml` (`[repos.<name>]`) or `TWIG__SECTION__KEY` env vars; `twig config get` shows only the user file. To change how a repo runs in one card, write the card's `.twig.toml` and `twig refresh` |
| `twig env [id] [--repo X]` | `export` lines with the full env of a herdr pane (ports, `PORT`, repo `env`, `TWIG_CARD_ID`). A pane herdr restored after a reboot has none of it (`$TWIG_CARD_ID` empty): run `eval "$(twig env)"` in it |
| `twig ports [id] [--env]` | the card's dev server ports. Never hardcode 4200/5000 in a card: use `$PORT` / `$TWIG_PORT_<REPO>` (already set in the herdr panes) or `eval "$(twig ports --env)"`. To pass them under the names the project reads (CORS origin, API URL), use the repo's `env` table (config or the repo's `.twig.toml`) instead of editing the project's code |
| `twig pr [id] [--draft] [-n]` | the work is ready for review: pushes each repo's branch and opens one PR per repo with commits ahead of the base (an open PR is just printed), linked to the card and to each other. Run `-n` first when unsure |
| `twig sync <id> [--merge\|--rebase] [-n]` | fetch the base branch and rebase (default, `git.sync`) or merge it into every repo of the card; skips repos with uncommitted changes, stops on a conflict naming repo/files; `-n` = dry run |
| `twig rm <id> [--branches] [-f]` | close the card and its herdr workspace; refuses with uncommitted changes or a running agent (`working`/`blocked`); `-f/--force` bypasses both, so only use it when the user asks |
| `twig memo ["text"] [-c id]` | **in development**: append a dated line to the card memory `MEMORY.md` (kept after `twig rm`); no text prints it. Format may change |
| `twig prune [-n]` | clean up orphaned worktree records (folder deleted by hand) |
| `twig doctor [--json]` | diagnose dependencies, login, config, and repos |
| `twig update` | update twig, its herdr plugin and this skill to the latest release |
| `twig uninstall` | remove twig (asks before touching config and dependencies; don't run it unless the user asks) |
| `twig new <id> --template NAME` | apply a card template from `templates.toml` (repos, agent, install, hooks); explicit flags override it; `[templates] default` in the config is used when omitted |
| `twig template ls` / `twig template show NAME` | list / inspect the templates; `twig template edit` opens an editor (interactive; don't run it as an agent) |
| `twig config edit` | edit the config (interactive, see below) |
| `twig config get <key>` / `twig config set <key> <value>` | read / change one config value by dotted key (`git.base_branch`, `repos.my-back.run`); validated, comments kept; arrays as `a,b` or `[a,b]` |

Lifecycle hooks (`[hooks]` and `[repos.<name>.hooks]` in the config: `post_create`, `pre_rm`, `post_rm`,
`post_open`, `post_refresh`) run the user's own shell commands around `new`/`add-repo`, `rm`, `open`/`herdr`/`code`
and `refresh`, with `TWIG_CARD_ID`, `TWIG_CARD_DIR`, `TWIG_REPO`, `TWIG_REPO_DIR`, `TWIG_BRANCH`, `TWIG_EVENT`,
the card's `TWIG_PORT_<REPO>` (and `PORT`, plus the repo's `env`, for repo hooks).
A failing `pre_rm` blocks `rm` (`-f` skips it); `post_*` failures only warn. `--no-hooks` (global) skips all of
them; don't use it unless the user asks. Never add hooks to the config on your own.

`--json` (on `ls`, `status`, `doctor`) prints only a JSON document on stdout, no colors; use it
instead of parsing the text. `ls`/`status`: `{"cards":[{"id","repos":[...]}]}` (`status` repos add
`dirty` count, `base_ref`/`vs_base`, `origin_ref`/`vs_origin`, `port` and a per-card `herdr`
`{workspace, agents_running}`); `doctor`: `{"ok","errors","warnings","sections":[{"name","checks":[{"level","message"}]}]}`
(still exits 1 on errors). Schemas in `docs/COMMANDS.md`.

`twig new` options:

| Option | Effect |
|---|---|
| `--template NAME` | preset from `templates.toml` (repos, agent, install, prefix/base, `autorun`, `agent_prompt`, hooks) |
| `--repos a,b` | the card's repos; without it, the template's, else those with `default = true` |
| `--branch NAME` | forces the branch in all repos |
| `--base NAME` | base for the new branch (default `git.base_branch`) |
| `--fix` | new branch with the `fix/` prefix |
| `--install` / `--no-setup` | runs / skips each repo's `install` and `post_create` |
| `--herdr` / `--code` | opens herdr / VS Code at the end |
| `--prompt TEXT` | first message of the agents started in herdr, e.g. a skill (`"/deliver {id}"`; `{id}`, `{repo}` replaced); `""` drops the template's `agent_prompt` |
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

- **`twig herdr <id>`, `twig open` (herdr target) and `--herdr` outside herdr open the herdr UI** and block the
  terminal. If `HERDR_ENV` is not `1`, don't use them; `twig new` already creates the
  herdr workspace on its own. Tell the user to run `twig herdr <id>`.
- **`twig config edit` opens an editor.** As an agent, prefer `twig config set <key> <value>`
  (validated, comments preserved, file untouched if the value is invalid) and `twig config get <key>`
  to read. For larger changes, edit the config file directly and validate with `twig doctor`.

## Choosing the right command

- "começa o card 123" (start card 123) → `twig new 123`. If the user named repos, `--repos`.
- "o card 123 também mexe no repo X" (card 123 also touches repo X) → `twig add-repo 123 X`.
- "fechei sem querer o painel do repo X" (accidentally closed repo X's pane) → `twig add-repo 123 X` (recreates what's missing).
- "abre o card 123 no browser" (open card 123's page) → `twig open-card 123`.
- "começa a issue 12 do repo web" (GitHub) → `twig new owner/web#12` (or `web#12` with `provider.gh.owner`).
- "abre os PRs do card" (open the card's PRs) → `twig pr <id> -n` to show what would happen, then
  `twig pr <id>` (`--draft` if the user wants drafts). Commit first: uncommitted changes don't go in.
- "limpa os cards que já terminaram" (clean up finished cards) → `twig clean -n`, show the list,
  and remove only what the user confirms (`twig clean` asks per card).
- "em que porta roda o front desse card?" (which port?) → `twig ports <id>`; inside a pane, `$PORT`.
- "o front desse card tem que chamar o back desse card" / CORS → the repo's `env` (in the config or
  its `.twig.toml`) mapping `$TWIG_PORT_<REPO>` to the setting the project reads; then
  `twig refresh <id>` and recreate the panes (close them, `twig herdr <id>`).
- "terminei o card 123" (finished card 123) → check `twig ls` (no `*`), confirm with the user, and only then
  `twig rm 123`. If it refuses because an agent is running, tell the user instead of using `-f`. Local branches are only deleted with `--branches`, and only if the user asks.
- "mudei de branch" or "mexi no config" (switched branch / changed the config) → `twig refresh <id>`.
- "dá erro de 'already registered'" ('already registered' error) → `twig prune`.
- "anota isso" / finished a chunk of work (note this / finished a chunk) → `twig memo "..."` (see "Card memory").
- Something odd in the environment → `twig doctor` and read the ✖ and ! lines.

## Rules inside a card

- **Work only inside `~/.wt/<id>/`.** Don't edit, check out, or commit in the main
  clones; they are shared by all cards.
- **Don't switch branches inside a worktree.** Another branch is another card.
- **Each repo commits separately.** There is no atomic commit across repos; mention the card
  (`#<id>`, or `owner/repo#N` on GitHub) in the messages on both sides when the change is coupled.
- **Don't hardcode ports.** Use `$PORT` / `$TWIG_PORT_<REPO>`; another card may be running the
  same repo on the default port.
- **Dependencies and caches are per worktree** (`node_modules`, `bin/`, `obj/`...). Don't
  symlink between cards.
- **The card's context file (`CLAUDE.md`/`AGENTS.md`) is not versioned.** Update the "Card context"
  section with decisions and loose ends; it survives `twig refresh`. The rest of the file
  is regenerated.

## Card memory (in development)

`twig memo` is experimental: use it, but expect the format and details to change. Each card
has a `MEMORY.md` in its root (a symlink to `<worktrees>/memos/<id>-<provider>/MEMORY.md`, so
it survives `twig rm`). The last 10 entries are also in the context file under "Memory".

- **When you enter a card, read `MEMORY.md`.**
- **Write to it with `twig memo "..."`** after finishing a chunk of work, after making a
  decision, and before stopping. One line each: what was done, what is left, or the decision
  and why. Newlines collapse; keep it short.
- Never edit the `memos/` folder by hand or treat it as a card.

## Config

Lives at `~/.config/twig/config.toml` (or the path in `TWIG_CONFIG`). Full format
in `docs/CONFIG.md` in the twig repository. The essentials:

```toml
version = 1

[agent]
provider = "claude"               # "claude" | "codex" (context file CLAUDE.md / AGENTS.md)
command  = ""                     # empty = the provider's default; what runs in the agent tab
# context_file = ""               # optional override (bare file name)

[provider]
default = "az"                    # "az" | "gh"

[vscode]
enabled     = true
open_on_new = false               # open VS Code at the end of new/add-repo

[open]
default = "herdr"                # target for `twig open` without one: "herdr" | "code"

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
port        = 5000                # base port; each card runs on 5000 + its offset (card 52644 → 5044)
ports       = { https = 5100 }    # more ports by name: TWIG_PORT_MY_BACKEND_HTTPS (bases ≥ span apart)
env         = { ASPNETCORE_URLS = "http://localhost:$PORT" }  # extra env, expanded per card
debug       = ["Debug: Back"]     # launch.json configs in the card's compound
local_files = ["src/*/appsettings.Development.json"]  # gitignored, copied from the clone

[ports]
span = 100                        # offsets go 1..=span; cards equal modulo span share ports
```

Layers on top of this file (strongest first): CLI flags > `TWIG__SECTION__KEY` env vars >
`<card>/.twig.toml` (`[repos.<name>]`) > the repo's versioned `.twig.toml` (`run`, `install`,
`port`, `env`...; no `path`/`default`/`hooks`) > this file. `twig config get/set` only see this
file.

To register a repo: add a `[repos.<name>]` block and run `twig doctor`.
A repo passed in `--repos` that isn't in the config yet is registered automatically if
the clone is found in `paths.repos` or `paths.search`.

## herdr: custom layouts

`twig` only creates the default layout (`agent` + `run`). For anything beyond that
(another tab, an extra pane, a standalone workspace, running and reading a command in
another pane), read [references/herdr.md](references/herdr.md) before running `herdr`
commands.
