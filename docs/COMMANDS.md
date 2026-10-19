# Commands

`twig` commands and options (`twig` is a working name).

## Summary

| Command | What it does |
|---|---|
| `twig new <id> [slug]` | creates the card: worktrees, herdr workspace and, if enabled, `.code-workspace` |
| `twig add-repo <id> <repo,...>` | adds more repos to an existing card |
| `twig open [id] [default\|herdr\|code]` | opens the card in the chosen target (default: `[open] default`, herdr) |
| `twig open-card [id]` (`opcd`) | opens the card's page in the browser (Azure DevOps only for now) |
| `twig herdr <id>` | opens the card's workspace in herdr (creates it if missing) |
| `twig code [id]` | opens the card in VS Code (generates the `.code-workspace` if missing); without an id, the current card |
| `twig ls [--json]` | lists active cards and each repo's branch |
| `twig status [id] [--json]` | per repo: branch, uncommitted files, ahead/behind vs base and origin; herdr workspace and agent |
| `twig refresh <id>` | regenerates `.code-workspace` and the context file (`CLAUDE.md` / `AGENTS.md`) |
| `twig sync <id> [--merge\|--rebase] [-n]` | fetches the base branch and rebases/merges it into every repo of the card |
| `twig rm <id>` | removes the card's worktrees, folder and herdr workspace |
| `twig prune` | cleans up orphaned worktree records |
| `twig doctor [--json]` | checks dependencies, login and config without changing anything |
| `twig config edit` | opens the config in an editor and validates it on save |
| `twig update [--check] [--version vX.Y.Z]` | updates twig, its herdr plugin and skill to the latest release |
| `twig uninstall [--yes]` | removes twig, its herdr plugin, skill and completions |
| `twig completions <shell>` | generates the completion script (bash, zsh, fish, powershell, elvish) |
| `twig help [command]` | general help or help for one command |

## Opening in herdr and VS Code

Rule shared by `twig new` and `twig add-repo`:

| | herdr | VS Code |
|---|---|---|
| Creates the structure | **always** (workspace with `agent` and `run` tabs) | only if `vscode.enabled = true` (generates the `.code-workspace`) |
| Opens at the end | if `herdr.open_on_new = true` **or** `--herdr` | if `vscode.open_on_new = true` **or** `--code` |
| Error | — | `--code` with `vscode.enabled = false` or without `code` installed |

You can open both, just one, or neither. See [CONFIG.md](CONFIG.md) → `[vscode]` and `[herdr]`.

## `twig new <id> [slug] [options]`

Creates the `<worktrees>/<id>/` folder with one worktree per repo, the card's context file (`CLAUDE.md`, or `AGENTS.md` with `agent.provider = "codex"`),
the herdr workspace and, if VS Code is enabled, the `.code-workspace`.

**Arguments**

| Argument | Description |
|---|---|
| `<id>` | card/issue id in the provider (`az` or `gh`) |
| `[slug]` | used only when the branch has to be **created**; without it, it comes from the card title |

**Options**

| Option | Description |
|---|---|
| `--repos a,b` | the card's repos; without it, uses the repos with `default = true` |
| `--branch NAME` | forces this branch in every repo |
| `--base NAME` | base for the new branch (default: `git.base_branch`) |
| `--fix` | new branch with the `fix/` prefix (a story that turned into a fix) |
| `--install` | runs each repo's `install` in the worktree |
| `--herdr` | opens herdr at the end |
| `--code` | opens VS Code at the end (error if VS Code isn't enabled/installed) |
| `--no-ado` | doesn't query the provider (offline) |

**Branch selection**, repo by repo:

1. `--branch`, if given.
2. A branch on origin whose name contains the `<id>`; if there are several, it asks.
3. Nothing found → creates `<prefix>/<id>-<slug>` from `origin/<base>`.
   The prefix comes from `git.branch_prefix` based on the work item type.

**What else happens**

- Fetches the card's title, type, state, tags, assignee, area, description, acceptance
  criteria and URL from the provider and caches them in the card folder.
- Generates the card's context file (`CLAUDE.md`, or `AGENTS.md` with `agent.provider = "codex"`): repos and branches, card data and instructions for the agent.
- Copies each repo's `local_files` from the main clone into the worktree.
- A repo that isn't in the config gets registered (see [CONFIG.md](CONFIG.md) → Repos).
- If something fails midway, rolls back the worktrees already created.

## `twig add-repo <id> <repo[,repo...]> [options]`

Adds more repos to an existing card. Safe to run again:

- Repo without a worktree in the card → creates it, with the same branch rule as `twig new`.
- Regenerates `.code-workspace` (if VS Code is enabled) and the context file.
- herdr: creates missing panes, including ones the user closed.

| Option | Description |
|---|---|
| `--branch NAME` | forces this branch in the new repos |
| `--base NAME` | base for the new branch |
| `--fix` | new branch with the `fix/` prefix |
| `--herdr` | opens herdr at the end |
| `--code` | opens VS Code at the end (error if VS Code isn't enabled/installed) |

## `twig open [id] [default|herdr|code]`

Opens a card in one target:

| Target | Same as |
|---|---|
| `herdr` | `twig herdr <id>` |
| `code` | `twig code [id]` |
| `default` (or omitted) | the target set in `[open] default` (`herdr` if unset) |

`twig herdr` and `twig code` are aliases that go through the same code path, with the same
options and errors. Without `<id>`, the card is inferred as in `twig code` (see below); with
the `herdr` target, a folder that isn't a card is an error (there is no workspace to open).
An unknown target is rejected. See [CONFIG.md](CONFIG.md) → `[open]`.

```bash
twig open 52644             # default target from the config
twig open 52644 code        # = twig code 52644
twig open                   # current card, default target
twig open code              # current card, in VS Code (a lone target is not read as an id)
```

## `twig open-card [id]` (alias `opcd`)

Opens the card's page (the Azure DevOps work item) in the default browser, using the URL the
provider cached for the card (fetched once from `az` if missing). Uses `xdg-open` on Linux and
`open` on macOS; if no opener is found, it prints the URL instead.

Without `<id>`, the card is inferred from the current directory; outside a card it fails with a
usage error. With `provider = "gh"` it exits non-zero ("not supported yet") until GitHub
`owner/repo#123` ids exist.

```bash
twig open-card 52644
twig opcd                   # current card
```

## `twig herdr <id>`

Opens the card's workspace in herdr. If it doesn't exist, creates:

- `agent` tab: one pane per repo running `agent.command` (`claude` or `codex` by default);
- `run` tab: one pane per repo that has `run` in the config, already starting the project.

If the workspace already exists, it just focuses it. To recreate a closed pane: `twig add-repo <id> <repo>`.

## `twig code [id]`

Opens the card in VS Code. If the `.code-workspace` doesn't exist yet, generates it before opening.
Error if `vscode.enabled = false` or if `code` isn't installed.

Without `<id>`, the card is inferred, in this order:

1. `workspace_label` from `HERDR_PLUGIN_CONTEXT_JSON` (set by herdr when a plugin runs), if it is a card id;
2. `focused_pane_cwd` / `workspace_cwd` from that context, or the current directory, if inside `<worktrees>/<id>/`;
3. not a card → opens that folder in VS Code.

This is what the herdr plugin in [herdr-plugins/](../herdr-plugins/README.md) calls on `prefix+shift+o`.
It strips `HERDR_*` from VS Code's environment so its terminal can run `herdr`.

## `twig ls`

Lists active cards and each repo's branch. A `*` marks uncommitted changes.

With `--json`, stdout is only this JSON document (no colors, no emoji, no update hint; an
empty list is `{"cards": []}`). `branch` is `null` on a detached HEAD.

```json
{
  "cards": [
    {
      "id": "123",
      "repos": [
        { "repo": "r1", "branch": "card-123", "dirty": true }
      ]
    }
  ]
}
```

## `twig status [id]`

A richer `ls`. Without an id, every card in compact form; with an id, that card plus the
refs compared against. Per repo:

- branch and `*N` (number of files with uncommitted changes, untracked included)
- `base ↑a ↓b`: commits ahead/behind `origin/<git.base_branch>` (the local base branch if
  there is no `origin/` copy)
- `origin ↑a ↓b`: commits ahead/behind the branch's upstream (or `origin/<branch>`);
  `-` when there is nothing to compare against

Per card: whether the herdr workspace exists and whether an agent is running
(`working`/`blocked`, same detection as `twig rm`). herdr is never started for this.
It reads local refs only (no fetch), so run `git fetch` for fresh numbers. Ports and PR
info are not shown yet.

With `--json`, stdout is only this JSON document (no colors, no emoji, no update hint).
It is always `{"cards": [...]}`: one entry per active card, or just the requested one with
`<id>` (an unknown card still fails with exit 1 and nothing on stdout). `base_ref`,
`vs_base`, `origin_ref` and `vs_origin` are `null` when there is nothing to compare against.

```json
{
  "cards": [
    {
      "id": "123",
      "repos": [
        {
          "repo": "r1",
          "branch": "card-123",
          "dirty": 1,
          "base_ref": "origin/main",
          "vs_base": { "ahead": 2, "behind": 0 },
          "origin_ref": "origin/card-123",
          "vs_origin": { "ahead": 1, "behind": 0 }
        }
      ],
      "herdr": { "workspace": true, "agents_running": 1 }
    }
  ]
}
```

`dirty` is the number of files with uncommitted changes; `agents_running` counts panes
that are `working` or `blocked`.

## `twig refresh <id> [--refetch]`

Regenerates the `.code-workspace` and context file of an existing card, preserving the
"Card context" section of the context file if it has been filled in (a section under the
old Portuguese heading `## Contexto do card` is also kept, and moved to the new heading).
Use it after renaming a branch or changing the config.

| Option | Description |
|---|---|
| `--refetch` | drops the cache and fetches the card data from the provider again |

## `twig rm <id> [--branches] [-f]`

Removes the card's worktrees and folder, and closes its herdr workspace. Refuses if any
repo has uncommitted changes or if an agent is running (`working` or `blocked`) in the
card's herdr workspace. Local branches are kept by default. Without herdr installed or
running, the herdr steps are skipped.

| Option | Description |
|---|---|
| `--branches` | also deletes the local branches |
| `-f`, `--force` | skips both checks (uncommitted changes and running agent) |

## `twig sync <id> [--merge | --rebase] [-n | --dry-run]`

Runs `git fetch origin <base>` in each repo of the card (`<base>` is `git.base_branch`) and brings
`origin/<base>` into the card branch, by rebase or merge.

- Repos with uncommitted changes (or a detached HEAD) are skipped and reported.
- A conflict stops the whole run, names the repo and the conflicting files, and leaves that
  worktree mid-rebase/merge for you to resolve (or abort with `git rebase --abort` / `git merge --abort`).
- A repo whose fetch fails is reported, the others continue, and the command exits 1 at the end.

| Option | Description |
|---|---|
| `--merge` | merge instead of rebasing |
| `--rebase` | rebase (overrides `git.sync = "merge"`) |
| `-n`, `--dry-run` | fetches, then only shows how many commits each repo would take; changes no worktree |

Without `--merge`/`--rebase`, the strategy is `git.sync` from the config (`"rebase"` by default).

## `twig prune [-n | --dry-run]`

Cleans up orphaned worktree records (folder deleted by hand instead of with `twig rm`) in
every repo in the config. Also lists folders under `<worktrees>` with no worktree inside
and asks before deleting them, since they may hold a context file (`CLAUDE.md` / `AGENTS.md`) with notes.

| Option | Description |
|---|---|
| `-n`, `--dry-run` | only shows what it would do |

## `twig doctor`

Checks the environment and shows what's ok (✔), warning (!) or error (✖), without changing anything.
Exits with 1 if there is at least one error; warnings don't affect the exit code.
Works even with a missing or invalid config (checks that depend on it are skipped).

| Item | How it checks | On failure |
|---|---|---|
| config | file exists and is valid | error |
| `git` | on the PATH | error |
| git identity | `git config --global user.name` / `user.email` | warning |
| `herdr` | on the PATH; if the server doesn't respond, it just reports it (twig starts it on its own) | error |
| VS Code | `code` on the PATH, only with `vscode.enabled = true` | error |
| provider `az` | on the PATH, `azure-devops` extension, real ADO access (`az devops project list`) | error / warning / warning |
| provider `gh` | on the PATH, `gh auth status` | error / warning |
| agent | first token of `agent.command` (`claude` / `codex`) on the PATH | error |
| folders | `paths.worktrees` exists or can be created | error |
| repos | each `[repos.x]` points to a git repo; at least one has `default = true` | error / warning |
| orphaned worktrees | same search as `twig prune -n` | warning |

With `--json`, stdout is only this JSON document (no colors, no emoji); the exit code is
the same as without it (1 on errors, the JSON is still printed first). `level` is one of
`ok`, `info`, `warn`, `error`; `ok` is `true` when there are no errors; section names and
messages are the human-readable ones and may change, the keys and levels are the stable part.

```json
{
  "ok": false,
  "errors": 1,
  "warnings": 0,
  "sections": [
    {
      "name": "config",
      "checks": [{ "level": "ok", "message": "/home/me/.config/twig/config.toml valid" }]
    },
    {
      "name": "herdr",
      "checks": [{ "level": "error", "message": "herdr not found on PATH (required)" }]
    }
  ]
}
```

ADO access is tested with a real call because `az account show` fails for
users of `az devops login` (PAT), even though `az boards` still works.

## `twig config edit`

Opens the config in `$VISUAL`, else `$EDITOR`, else `nano` (Windows: `notepad`). The value
may include arguments (e.g. `EDITOR="code --wait"`). Since it follows the symlink, it edits the
real file.

- Config doesn't exist → creates it from a minimal template before opening.
- When the editor closes, it validates. Invalid → shows the error and asks `reopen to fix it? [Y/n]`.
  Without a terminal, an empty answer counts as "no", to avoid looping.
- Exits with 1 if the file is left invalid.

## `twig update [--check] [--version vX.Y.Z]`

Asks the GitHub API for the latest release (`TWIG_REPO`, default `RafaelKC/twig`; uses
`GITHUB_TOKEN` if set). If it's newer than the running version, downloads that tag's
`install.sh` and runs it with `--update`: twig, the AI skill, the herdr plugin and
completions are replaced; dependencies and config are not touched. Needs `curl`.

| Option | Description |
|---|---|
| `--check` | only reports whether a new version exists |
| `--version vX.Y.Z` | installs that version, even if it's older or the same |

Other commands also hint at a new release, on stderr, at most once a day; see `[update]` in
[CONFIG.md](CONFIG.md) and `TWIG_NO_UPDATE_CHECK=1` to turn it off.

## `twig uninstall [--yes]`

Removes what the installer put in place:

- the herdr plugin (`herdr plugin unlink twig`, `~/.local/share/twig`) and the shortcut
  block between `# twig:begin` / `# twig:end` in herdr's `config.toml`;
- the AI skill `~/.claude/skills/twig` or `~/.agents/skills/twig` (only if it's twig's);
- shell completions and the `PATH` line the installer added to `~/.bashrc` / `~/.zshrc`;
- the `twig` binary on the `PATH`.

Then asks (default no) whether to also remove the config and `az`, `gh` and `herdr`.
Tools are removed the way they were installed: Homebrew, apt, dnf or pacman when one of
them owns the package; otherwise the binary file, if it's under the home directory.
Card folders (`paths.worktrees`) are never touched; run `twig rm` first if you want.

| Option | Description |
|---|---|
| `-y`, `--yes` | don't ask: removes twig's pieces, keeps the config and the dependencies |

Without a terminal and without `--yes`, it cancels.

## `twig help [command]`

General help, or detailed help for one command. Every command also accepts `-h` / `--help`.

## Outside the CLI

| Item | What it does |
|---|---|
| Completions | commands and options (`twig completions <shell>`) |
| herdr plugin | `open-code` action (`prefix+shift+o`): opens the focused card in VS Code |
| Claude Code skill | `skills/twig/`: teaches the agent the card workflow and custom herdr layouts |

## Open questions

- `--no-ado` with the `gh` provider: rename to something generic (e.g. `--offline`).
- `--fix`: generalize to `--prefix NAME`, since prefixes come from the config.
