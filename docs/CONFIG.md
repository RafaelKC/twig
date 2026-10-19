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
owner = ""                        # to resolve "#123" without owner/repo

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
run         = "dotnet watch run --project src/Api"
debug       = ["Debug: Api"]
local_files = ["src/*/appsettings.Development.json"]

[repos.web]
label   = "🌐 Web"
short   = "web"
default = true
run     = "pnpm start"
```

## Changing it from the command line

`twig config get <key>` and `twig config set <key> <value>` read and write one value by dotted
key (`git.base_branch`, `repos.my-back.run`), validated with the same rules as `twig config edit`
and keeping comments and formatting. Details: [COMMANDS.md](COMMANDS.md#twig-config-get-key--twig-config-set-key-value).
Per-repo `.twig.toml`, per-card overrides and env overrides are not implemented yet
([#19](https://github.com/RafaelKC/twig/issues/19)).

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
| `gh.owner` | `""` | default owner to resolve `#123` without `owner/repo` |

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

- Per-repo override (versioned file such as `.twig.toml`) and via environment variable,
  and the precedence order between them.
- Per-project-type strategies (run and ports), once the `type` field comes back.
