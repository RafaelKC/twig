# Config

Format of the `twig` config file. Still a proposal: none of this is
implemented.

## Location

| OS | Path |
|---|---|
| Linux / WSL / macOS | `~/.config/twig/config.toml` |
| Windows | `%APPDATA%\twig\config.toml` |

**TOML** format: it's what herdr and Cargo use, and it avoids YAML's indentation
and type-coercion pitfalls in a hand-edited file.

## Full example

```toml
version = 1                       # schema version, so `twig update` can migrate

[agent]
provider = "claude"               # "claude" | "codex"
# command      = "claude"         # what `twig herdr` runs in the agent tab (default: the provider's)
# context_file = "CLAUDE.md"      # card context file (default: CLAUDE.md / AGENTS.md)

[provider]
default = "az"                    # "az" | "gh"

[provider.az]
organization = "https://dev.azure.com/my-org"   # empty = use the az default
project      = "MyProject"
item_url     = ""                 # manual fallback, {id} becomes the number

[provider.gh]
owner = ""                        # so "repo#123" means "<owner>/repo#123"

[vscode]
enabled     = true
open_on_new = true                # open VS Code when creating a card

[herdr]
open_on_new = false               # create/open the herdr workspace when creating a card

[paths]
worktrees = "~/.wt"
repos     = "~"                   # root of the main clones
search    = ["~/dev", "~/src"]    # fallbacks if the repo isn't under `repos`

[git]
base_branch   = "main"
branch_prefix = { Bug = "fix", "User Story" = "feat", default = "feat" }

[repos.api]
path        = ""                  # empty = <paths.repos>/api
label       = "⚙️ API"            # name in VS Code
short       = "api"               # tab/pane name in herdr
default     = true                # included when --repos isn't passed
run         = "dotnet watch run --project src/Api --urls http://localhost:$PORT"
port        = 5000                # base port; each card gets its own (see [ports])
debug       = ["Debug: Api"]
local_files = ["src/*/appsettings.Development.json"]

[repos.web]
label   = "🌐 Web"
short   = "web"
default = true
run     = "pnpm start -- --port $TWIG_PORT_WEB"
port    = 4200
```

## Changing it from the command line

`twig config get <key>` and `twig config set <key> <value>` read and write one value by dotted
key (`git.base_branch`, `repos.my-back.run`), validated with the same rules as `twig config edit`
and keeping comments and formatting. Details: [COMMANDS.md](COMMANDS.md#twig-config-get-key--twig-config-set-key-value).
They work on the file only: the [overrides](#overrides-and-precedence) below are not shown or
changed by them.

## Overrides and precedence

On top of this file, three layers can change settings. From strongest to weakest:

1. **CLI flags** (`--base`, `--repos`, `--merge`...), for one command;
2. **environment variables** `TWIG__<SECTION>__<KEY>`;
3. **the card's `.twig.toml`** (`<worktrees>/<id>/.twig.toml`);
4. **a repo's `.twig.toml`**, versioned in the repo;
5. **this file** (the user config), then the built-in defaults.

Every layer goes through the same validation as the config file, so a bad value is an error
that names the file or the variable.

### A repo's `.twig.toml`

A `.twig.toml` at the root of a repo, committed with it, sets **that repo's** settings for
everyone on the team, so each person's config doesn't have to repeat them. Keys at the root:

```toml
# .twig.toml in the repo
run         = "ng serve --port ${TWIG_PORT_WEB:-4200}"
install     = "pnpm install"
port        = 4200
local_files = [".env.local"]
```

Allowed: `label`, `short`, `run`, `autorun`, `port`, `install`, `post_create`, `local_files`,
`debug`. Not allowed: `path` and `default` (they are about your machine and your cards) and
`hooks` (personal workflow, kept in your own config). In a card, the file is read from the
card's **worktree**, so a branch that changes it takes effect in that card only; outside cards
(e.g. `twig doctor`) from the main clone.

### A card's `.twig.toml`

`<worktrees>/<id>/.twig.toml`, written by hand, changes repo settings for one card only (it is
not versioned, and `twig rm` deletes it with the card). Only `[repos.<name>]` tables, with the
same keys as a repo's file:

```toml
[repos.api]
run  = "dotnet run --launch-profile Mock"
port = 6000
```

### Environment variables

`TWIG__<SECTION>__<KEY>=value` overrides any key of this file, for whatever runs with it (a
shell, a CI job, a herdr pane). Segments are separated by a double underscore and matched
against the existing keys ignoring case and treating `_` like `-`, so repo names with dashes
work. Values are read like `twig config set` reads them (numbers, booleans, `a,b` lists).

```bash
TWIG__GIT__BASE_BRANCH=develop twig new 123     # this card branches from develop
export TWIG__REPOS__MY_API__RUN="dotnet watch"  # repos.my-api.run
export TWIG__VSCODE__ENABLED=false
```

`twig doctor` lists the variables in effect and validates each repo's `.twig.toml` in its main
clone.

## Fields

### Root

| Field | Required | Default | Description |
|---|---|---|---|
| `version` | yes | `1` | schema version; `twig update` uses it to migrate |

### `[agent]`

| Field | Default | Description |
|---|---|---|
| `provider` | `"claude"` | agent chosen at install: `claude` (Claude Code) or `codex` (Codex). Anything else is rejected |
| `command` | the provider's (`claude` / `codex`) | command that starts the agent in herdr's `agent` tab |
| `context_file` | the provider's (`CLAUDE.md` / `AGENTS.md`) | name of the card's context file; a file name, not a path |

The section used to be called `[ai]`; the old name is still accepted.

### `[provider]`

| Field | Default | Description |
|---|---|---|
| `default` | chosen at install | provider used when the card doesn't make it clear (`az` or `gh`) |
| `az.organization` | `""` | ADO org; empty = `az devops configure` default |
| `az.project` | `""` | ADO project; empty = `az` default |
| `az.item_url` | `""` | manual work item URL (`{id}` becomes the number), used only if `az` doesn't return one |
| `gh.owner` | `""` | default owner, so `repo#123` means `<owner>/repo#123` (see [Card ids](COMMANDS.md#card-ids)) |

### `[vscode]`

| Field | Default | Description |
|---|---|---|
| `enabled` | chosen at install | `false` disables `twig code` and `.code-workspace` generation |
| `open_on_new` | `true` | opens VS Code at the end of `twig new` / `twig add-repo`; ignored if `enabled = false` |

### `[herdr]`

| Field | Default | Description |
|---|---|---|
| `open_on_new` | `false` | focuses the card's herdr workspace at the end of `twig new` / `twig add-repo` |

The herdr workspace is **always created** by `twig new`; `open_on_new` only decides
whether it gets opened. The `.code-workspace` is only generated with `vscode.enabled = true`.

The two `open_on_new` settings are independent: you can open both, just one, or neither.
With `open_on_new = false`, the user opens it for that run with `--herdr` or `--code`.
`--code` errors if `vscode.enabled = false` or if `code` isn't installed.
See [COMMANDS.md](COMMANDS.md).

### `[open]`

| Field | Default | Description |
|---|---|---|
| `default` | `"herdr"` | target of `twig open` when none is given: `"herdr"` or `"code"` |

Any other value is a config error (reported by `twig doctor` and `twig config edit`).
`"code"` with `vscode.enabled = false` is flagged by `twig doctor`.

### `[update]`

| Field | Default | Description |
|---|---|---|
| `check` | `true` | after a command, prints `A new version of twig is available (v0.2.0 → v0.3.0). Run: twig update` to **stderr** |
| `check_interval_hours` | `24` | how often the latest release is looked up; the result is cached in `~/.cache/twig/latest-version` (`$XDG_CACHE_HOME`) |

The check never slows or breaks a command: it uses the cache, refreshes it with a 2 s
timeout, and any failure (offline, rate limit) is silent. The hint is shown at most once per
new version per day, only when stdout and stderr are terminals, and never for `update`,
`uninstall`, `completions`, `doctor` or `config edit`. Set `TWIG_NO_UPDATE_CHECK=1` to disable
it from the environment. `twig doctor` reports the setting.

### `[paths]`

| Field | Default | Description |
|---|---|---|
| `worktrees` | `~/.wt` (Windows: `%USERPROFILE%\.wt`) | where card folders live |
| `repos` | chosen at install | root of the main clones |
| `search` | `[]` | other folders to search for a repo that isn't under `repos` |

`~` expands to the user's home on every OS.

### `[git]`

| Field | Default | Description |
|---|---|---|
| `base_branch` | `"main"` | branch new branches are created from |
| `branch_prefix` | `{ default = "feat" }` | new branch prefix per work item type; `default` for the rest |
| `sync` | `"rebase"` | how `twig sync` brings the base branch in: `"rebase"` or `"merge"` (overridden by `--rebase`/`--merge`) |

### `[ports]`

Per-card dev server ports, so two cards on the same repo can run at the same time. A repo
opts in with `port = <base>` in `[repos.<name>]`; in a card it gets `base + offset`.

| Field | Default | Description |
|---|---|---|
| `span` | `100` | the offset is `1 + (card number - 1) % span`, so it is in `1..=span` |

The offset is **deterministic**: it only depends on the card number (ids that aren't numbers
use a stable hash of the id), so the same card always gets the same ports, after `rm` + `new`
or on another machine, and there is no allocation state to keep. With the default span, card
`52644` gets offset `44`: `ui` with `port = 4200` runs on `4244`, `api` with `port = 5000` on
`5044`. The offset is never `0`, so a card never takes the base port of the main clone.

Two cards whose numbers are equal modulo `span` share ports: `new`/`add-repo`/`refresh` warn
about it (and about a port some other process is already listening on), but don't change them.

The ports reach the processes as **environment variables**, and the project's `run` command
(or its own config) decides how to use them, falling back to its default when they are unset:

| Variable | Value |
|---|---|
| `TWIG_PORT_<REPO>` | the port of each repo of the card (name upper-cased, non-alphanumerics as `_`: `my-api` → `TWIG_PORT_MY_API`), so a front end can point at the same card's back end |
| `PORT` | the repo's own port (many dev servers read it by themselves) |

They are set on every herdr pane twig creates for the card (`agent` and `run` tabs), on the
`install`/`post_create` commands and hooks, on the VS Code tasks (`run: <repo>`, `PORT` too) and
terminals, and written to `<card>/.twig-ports.env`. Outside those, `twig ports --env` prints
`export` lines. A herdr pane that already existed keeps its old environment: close it and run
`twig herdr <id>` to recreate it.

```toml
[repos.web]
run  = "ng serve --port ${TWIG_PORT_WEB:-4200}"
port = 4200

[repos.api]
run  = "dotnet run --urls http://localhost:${PORT:-5000}"
port = 5000
```

### `[worktree]`

| Field | Default | Description |
|---|---|---|
| `local_files` | `[]` | gitignored files copied into **every** repo of the card (added to each repo's `local_files`) |
| `auto_install` | `false` | `twig new` / `add-repo` run each new repo's `install` and `post_create` on their own, before herdr starts the `run` panes. `--no-setup` skips it for one call; `--install` forces it |

### `[hooks]` and `[repos.<name>.hooks]`

Lifecycle hooks: lists of shell commands you wrote, run around card operations. `[hooks]` is
card-wide; `[repos.<name>.hooks]` is per repo. Both take the same five keys, each a list of
non-empty strings (`twig doctor` flags empty ones):

| Key | When | Failure |
|---|---|---|
| `post_create` | after `twig new` / `add-repo` created the worktrees (and after `install`/legacy `post_create`, if they ran) | warns |
| `pre_rm` | `twig rm`, after the safety checks, before anything is removed | **aborts** the rm; skipped with `-f/--force` |
| `post_rm` | after the card was removed | warns |
| `post_open` | after `twig open` / `herdr` / `code` (and `new --herdr/--code`) opened the card | warns |
| `post_refresh` | after `twig refresh` | warns |

```toml
[hooks]
pre_rm       = ["./scripts/stop-stack.sh"]
post_open    = ["notify-send 'card ready'"]

[repos.api.hooks]
post_create  = ["dotnet ef database update"]
```

- **Where they run.** Card-level hooks run in the card folder; repo-level hooks run in that repo's
  worktree, once per repo of the card (for `new`/`add-repo`, only the repos just created). Exception:
  `post_rm`, where the folders are gone: card-level runs in `paths.worktrees`, repo-level in the
  repo's main clone.
- **Order.** Per list, commands run in order and stop at the first failure. Across lists:
  `pre_*` runs card-level first, then each repo; `post_*` runs each repo, then card-level.
- **Environment.** `TWIG_CARD_ID` (also `TWIG_CARD`), `TWIG_CARD_DIR`, `TWIG_REPO`,
  `TWIG_REPO_DIR` (the worktree), `TWIG_BRANCH` and `TWIG_EVENT` (`post_create`, `pre_rm`, ...).
  In card-level hooks `TWIG_REPO`, `TWIG_REPO_DIR` and `TWIG_BRANCH` are empty.
- **Output** streams to the terminal; there is no timeout.
- **`--no-hooks`** (global flag, any command) skips every hook for that invocation.
- **Your commands only.** Hooks run exactly what you wrote in your own config: nothing is
  injected by a provider, card or plugin. This is the safe part of
  [#5](https://github.com/RafaelKC/twig/issues/5) (running an agent command after creating a
  workspace): put it in `post_create` yourself.
- The per-repo `post_create` field below is unchanged (it only runs with `--install` /
  `worktree.auto_install`, after `install`); the hook `post_create` always runs.

### `[templates]` and `templates.toml`

Card templates are named presets for `twig new <id> --template NAME`. They live in their **own
file**, `~/.config/twig/templates.toml` (next to `config.toml`; `TWIG_TEMPLATES` overrides the
path). The file is optional: without it nothing changes. The config only holds the optional default:

```toml
[templates]
default = "fullstack"   # used by `twig new` when --template is omitted
```

`templates.toml` has one table per template (see [`examples/templates.toml`](../examples/templates.toml)):

```toml
[fullstack]
repos         = ["api", "ui"]   # repos from [repos.*]; used when --repos is not given
agent         = "claude"        # "claude" | "codex": context file and agent command for the card
install       = true            # run each repo's install/post_create (false = skip, like --no-setup)
branch_prefix = "feat"          # optional; replaces git.branch_prefix (--fix still wins)
base          = "main"          # optional; replaces git.base_branch

[fullstack.hooks]               # same keys as [hooks]; only for cards created from this template
post_create = ["echo created $TWIG_CARD_ID"]
```

- **Precedence.** Explicit flags (`--repos`, `--branch`, `--base`, `--install`/`--no-setup`, `--fix`)
  beat the template, which beats the config.
- **Hooks.** Template hooks are card-level and run **after** the config's `[hooks]` of the same event.
- **Remembered.** The template name is stored in the card folder (`.twig-template`), so `refresh`,
  `add-repo`, `open`, `rm`... keep using its hooks, agent, `install`, `branch_prefix` and `base`.
- **Validation.** `twig doctor` and `twig template edit` report empty names, repos that are not in the
  config, an unknown `agent`, empty hook commands and a `templates.default` that does not exist.
- Choosing the template automatically from the work item type/tags is not supported yet
  (follow-up on [#14](https://github.com/RafaelKC/twig/issues/14)).

### `[repos.<name>]`

`<name>` is what the repo is called in `--repos`.

| Field | Default | Description |
|---|---|---|
| `path` | `<paths.repos>/<name>` | path to the main clone, for repos outside the root |
| `label` | `<name>` | display name in the VS Code workspace |
| `short` | `<name>` | short tab/pane name in herdr |
| `default` | `false` | included in the card when the user doesn't pass `--repos` |
| `run` | — | command to start the project: becomes a VS Code task and a pane in herdr's `run` tab |
| `autorun` | `true` | `false` = don't start it in herdr's `run` tab (still a VS Code task); only applies with `run` |
| `port` | — | base dev server port; each card gets `port + offset` as `TWIG_PORT_<REPO>` / `PORT` (see [`[ports]`](#ports)) |
| `install` | — | dependency install command (e.g. `pnpm install`), used by `--install` or `worktree.auto_install`; without it, the repo is skipped |
| `post_create` | `[]` | extra shell commands run in the worktree after `install` (e.g. `["cp .env.example .env", "dotnet ef database update"]`). Env: `TWIG_CARD`, `TWIG_REPO`. Stops at the first failure (warns only) |
| `hooks` | — | per-repo lifecycle hooks, see [`[hooks]`](#hooks-and-reposnamehooks) |
| `debug` | `[]` | exact names of `.vscode/launch.json` configs included in the card's compound |
| `local_files` | `[]` | gitignored files (globs allowed, including `**`) copied from the main clone into the worktree |

## Repos

The installer does **not** register repos. It only sets `paths.repos`.

### Repo not in the config

When the user passes `--repos x` and `x` isn't registered, `twig` registers it:

1. Looks for the `x` folder in `paths.repos`, then in `paths.search`.
2. Found → adds `[repos.x]` to the config with defaults (only `path`, if it's
   outside `paths.repos`) and reports what was registered.
3. Not found → asks for the clone path. If the user doesn't provide one, it aborts.

### Configuring repos

1. Open the config:
   ```bash
   twig config edit        # opens in $EDITOR
   ```
2. Add one section per repo:
   ```toml
   [repos.my-backend]
   label   = "⚙️ Back"
   short   = "back"
   default = true
   run     = "dotnet watch run"
   ```
3. If the clone is outside `paths.repos`, set its path:
   ```toml
   path = "~/other-folder/my-backend"
   ```
4. Set `default = true` on the repos that should be in every `twig new` without `--repos`.
5. Check with `twig doctor`: it validates that each `path` exists and is a git repo.

## Open questions

- Per-project-type strategies (detect Angular/.NET/Node and set the port without touching
  `run`), once the `type` field comes back.
