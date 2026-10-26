# Commands

`twig` commands and options (`twig` is a working name).

## Summary

| Command | What it does |
|---|---|
| `twig new <id> [slug]` | creates the card: worktrees, herdr workspace and, if enabled, `.code-workspace` |
| `twig add-repo <id> <repo,...>` | adds more repos to an existing card |
| `twig template ls` / `show NAME` / `edit` | lists, shows and edits the card templates (`templates.toml`) |
| `twig open [id] [default\|herdr\|code]` | opens the card in the chosen target (default: `[open] default`, herdr) |
| `twig open-card [id]` (`opcd`) | opens the card's page (work item or issue) in the browser |
| `twig herdr <id>` | opens the card's workspace in herdr (creates it if missing) |
| `twig code [id]` | opens the card in VS Code (generates the `.code-workspace` if missing); without an id, the current card |
| `twig ls [--json]` | lists active cards and each repo's branch |
| `twig status [id] [--json]` | per repo: branch, uncommitted files, ahead/behind vs base and origin; herdr workspace and agent |
| `twig ports [id] [--env]` | shows the card's dev server ports; `--env` prints `export` lines |
| `twig env [id] [--repo X]` | prints `export` lines with the full environment of a herdr pane (ports, `PORT`, repo `env`, markers) |
| `twig refresh <id>` | regenerates `.code-workspace` and the context file (`CLAUDE.md` / `AGENTS.md`) |
| `twig memo [text] [-c id]` (in development) | appends a dated line to the card memory (`MEMORY.md`, survives `rm`); without text, shows it. See [MEMORY.md](MEMORY.md) |
| `twig sync <id> [--merge\|--rebase] [-n]` | fetches the base branch and rebases/merges it into every repo of the card |
| `twig pr [id] [--draft] [-n]` | pushes each repo's branch and opens one linked PR per repo (`gh` / `az repos`) |
| `twig rm <id>` | removes the card's worktrees, folder and herdr workspace |
| `twig clean [-n] [-y] [--branches] [-f]` | removes finished cards (branches merged or work item closed), asking per card |
| `twig prune` | cleans up orphaned worktree records |
| `twig doctor [--json]` | checks dependencies, login and config without changing anything |
| `twig config edit` | opens the config in an editor and validates it on save |
| `twig config get <key>` / `twig config set <key> <value>` | reads / writes one config value by dotted key, validated, comments preserved |
| `twig update [--check] [--version vX.Y.Z]` | updates twig, its herdr plugin and skill to the latest release |
| `twig uninstall [--yes]` | removes twig, its herdr plugin, skill and completions |
| `twig completions <shell>` | prints the shell completion hook (bash, zsh, fish, powershell, elvish) |
| `twig help [command]` | general help or help for one command |

## Global options

| Option | Description |
|---|---|
| `--no-hooks` | skips every lifecycle hook (`[hooks]`, `[repos.<name>.hooks]`) for this invocation; see [CONFIG.md](CONFIG.md) |

Lifecycle hooks fire from `new`/`add-repo` (`post_create`), `rm` (`pre_rm`, `post_rm`),
`open`/`herdr`/`code` (`post_open`) and `refresh` (`post_refresh`). A failing `pre_*` hook aborts
the command; a failing `post_*` hook only warns.

## Short aliases

Frequently typed commands and flags have short forms (listed by `--help` and offered by the
completions). The long forms keep working. Short flags are per command: the same letter can mean
different flags on different commands (e.g. `-r` is `--repos`, `--refetch` or `--rebase`, and `-b`
is `--branch` or `--branches`), so check the table or `--help` of the command you are using.

| Command | Alias |
|---|---|
| `add-repo` | `ar` |
| `status` | `st` |
| `open-card` | `opcd` |

| Flag | Short | Commands |
|---|---|---|
| `--template` | `-t` | `new` |
| `--repos` | `-r` | `new` |
| `--branch` | `-b` | `new`, `add-repo` |
| `--branches` | `-b` | `rm`, `clean` |
| `--base` | `-B` | `new`, `add-repo` |
| `--fix` | `-F` | `new`, `add-repo` |
| `--install` / `--no-setup` | `-i` / `-S` | `new`, `add-repo` |
| `--herdr` / `--code` | `-H` / `-c` | `new`, `add-repo` |
| `--no-ado` / `--prompt` | `-o` / `-p` | `new` |
| `--json` | `-j` | `ls`, `status`, `doctor` |
| `--env` | `-e` | `ports` |
| `--refetch` | `-r` | `refresh` |
| `--merge` / `--rebase` | `-m` / `-r` | `sync` |
| `--draft` | `-d` | `pr` |
| `--check` | `-c` | `update` |
| `--dry-run`, `--force`, `--yes` | `-n`, `-f`, `-y` | as before |

## Opening in herdr and VS Code

Rule shared by `twig new` and `twig add-repo`:

| | herdr | VS Code |
|---|---|---|
| Creates the structure | **always** (workspace with `agent` and `run` tabs) | only if `vscode.enabled = true` (generates the `.code-workspace`) |
| Opens at the end | if `herdr.open_on_new = true` **or** `--herdr` | if `vscode.open_on_new = true` **or** `--code` |
| Error | — | `--code` with `vscode.enabled = false` or without `code` installed |

You can open both, just one, or neither. See [CONFIG.md](CONFIG.md) → `[vscode]` and `[herdr]`.

## Card ids

Every command that takes a card id accepts what the provider calls it:

| Provider | You type | Card folder / herdr workspace | Branch |
|---|---|---|---|
| `az` | `52644` or `52644-some-slug` | `52644` | `feat/52644-<slug>` |
| `gh` | `123` (looked up in the card's first repo) | `123` | `feat/123-<slug>` |
| `gh` | `owner/repo#123`, `repo#123` (owner from `provider.gh.owner`) or `https://github.com/owner/repo/issues/123` | `repo-123` | `feat/123-<slug>` |

GitHub issue numbers belong to a repository, so the same number can be two different issues in
two repos. A repo-scoped id keeps them apart: the card folder is named `<repo>-<number>`, the
issue is always fetched with `gh issue view <number> --repo owner/repo`, and the card remembers
it (`.twig-issue` in the card folder). After `new`, both the folder name and the full id work:
`twig status tracker-12` and `twig status acme/tracker#12` are the same card.

`owner/repo#123` ids are rejected with `provider.default = "az"`. Quoting is not needed in bash
or zsh (`#` only starts a comment at the start of a word).

## `twig new <id> [slug] [options]`

Creates the `<worktrees>/<id>/` folder with one worktree per repo, the card's context file (`CLAUDE.md`, or `AGENTS.md` with `agent.provider = "codex"`),
the herdr workspace and, if VS Code is enabled, the `.code-workspace`.

**Arguments**

| Argument | Description |
|---|---|
| `<id>` | card/issue id in the provider (`az` or `gh`); see [Card ids](#card-ids) |
| `[slug]` | used only when the branch has to be **created**; without it, it comes from the card title |

**Options**

| Option | Description |
|---|---|
| `--template NAME` | applies a preset from `templates.toml` (repos, agent, install, branch prefix/base, hooks); without it, `[templates] default`. Explicit flags below override it |
| `--repos a,b` | the card's repos; without it, the template's, else the repos with `default = true` |
| `--branch NAME` | forces this branch in every repo |
| `--base NAME` | base for the new branch (default: `git.base_branch`) |
| `--fix` | new branch with the `fix/` prefix (a story that turned into a fix) |
| `--install` | runs each repo's `install` in the worktree |
| `--herdr` | opens herdr at the end |
| `--code` | opens VS Code at the end (error if VS Code isn't enabled/installed) |
| `--no-ado` | doesn't query the provider (offline) |
| `--prompt TEXT` | first message of the agents started in herdr, e.g. a skill: `--prompt "/deliver {id}"` (`{id}`, `{repo}` replaced); overrides the template's `agent_prompt`, `""` = none |

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

Opens the card's page (the Azure DevOps work item or the GitHub issue) in the default browser,
using the URL the provider cached for the card (fetched once from `az`/`gh` if missing). Without a
cached URL it falls back to `provider.az.item_url` (Azure DevOps) or
`https://github.com/<owner>/<repo>/issues/<n>` for a [repo-scoped](#card-ids) GitHub card. Uses
`xdg-open` on Linux and `open` on macOS; if no opener is found, it prints the URL instead.

Without `<id>`, the card is inferred from the current directory; outside a card it fails with a
usage error.

```bash
twig open-card 52644
twig opcd                   # current card
```

## `twig herdr <id>`

Opens the card's workspace in herdr. If it doesn't exist, creates:

- `agent` tab: one pane per repo running `agent.command` (`claude` or `codex` by default);
- `run` tab: one pane per repo that has `run` in the config, already starting the project.

Every pane it creates gets `TWIG_CARD_ID`/`TWIG_PANE_REPO`, the card's ports (`TWIG_PORT_<REPO>`, `PORT`) and the repo's `env` as
environment variables (see [CONFIG.md → `[ports]`](CONFIG.md#ports)). On `twig new`, the agents
can start with a first message (`--prompt` / the template's `agent_prompt`).

If the workspace already exists, it just focuses it and recreates the panes that are missing
(a closed pane comes back; `twig add-repo <id> <repo>` does the same). An existing pane keeps the
environment it was created with: after changing `port`/`env`, close it and run `twig herdr <id>`
(or `eval "$(twig env)"` in it; see [`twig env`](#twig-env-id---repo-repo---auto)).

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
`vs_base`, `origin_ref` and `vs_origin` are `null` when there is nothing to compare against;
`port` is the card's dev server port for the repo (`null` without `port` in the config) and
`ports` its named ports (`{"https": 5144}`, `{}` without `ports`).

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
          "vs_origin": { "ahead": 1, "behind": 0 },
          "port": null,
          "ports": {}
        }
      ],
      "herdr": { "workspace": true, "agents_running": 1 }
    }
  ]
}
```

`dirty` is the number of files with uncommitted changes; `agents_running` counts panes
that are `working` or `blocked`.

## `twig ports [id] [--env]`

Shows the card's dev server ports: one line per repo with `port` in the config, with its
variable and whether something is already listening on it. The ports are deterministic per
card (see [CONFIG.md → `[ports]`](CONFIG.md#ports)). Without `<id>`, the card of the current
directory.

`--env` prints `export TWIG_PORT_<REPO>=<port>` lines instead, for a terminal outside herdr:

```bash
twig ports 52644
eval "$(twig ports --env)"     # inside the card folder
```

## `twig env [id] [--repo <repo>] [--auto]`

Prints `export` lines with everything a herdr pane of the card receives: `TWIG_CARD_ID` and
`TWIG_PANE_REPO` (markers that tell a pane set up by twig from a bare shell), every `TWIG_PORT_*`,
`PORT` and the repo's `env`, resolved through all the config layers (user config, the repo's
`.twig.toml`, the card's `.twig.toml`, `TWIG__*`). Without `<id>` it uses the card of the current
directory, and the repo too when inside its worktree.

```bash
eval "$(twig env)"                    # inside a card's repo folder, in any terminal
eval "$(twig env 52644 --repo api)"
```

herdr restores a card's layout after a reboot, but its panes are fresh shells without the
environment twig injected (the `dotnet run` that needed `ASPNETCORE_ENVIRONMENT` starts as
`Production`). Two ways out: run `eval "$(twig env)"` in the pane, or close the panes and run
`twig herdr <id>`. `install.sh` also adds a line to `~/.bashrc` / `~/.zshrc` that does it
automatically: in a herdr pane (`HERDR_ENV=1`) without `TWIG_CARD_ID`, it runs `twig env --auto`,
which prints nothing unless the directory is inside a card. The shell checks both variables
before starting twig, so terminals outside herdr pay nothing. `twig doctor` warns when run in
such a pane. The environment is recomputed from the current config, never saved, so no secrets
are stored. PowerShell and fish profiles are not touched: use `twig env` by hand there. Limits: the hook only helps when the shell starts inside a
card folder, and it cannot relaunch `run`; there is no saved per-pane snapshot, so a pane
restored elsewhere needs `twig herdr <id>`.

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
running, the herdr steps are skipped. The card's memo (see `twig memo`) is never removed;
when one exists, `rm` prints where it is kept.

| Option | Description |
|---|---|
| `--branches` | also deletes the local branches |
| `-f`, `--force` | skips both checks (uncommitted changes and running agent) and the `pre_rm` hooks |

Hooks: `pre_rm` runs after the checks and aborts the removal if it fails; `post_rm` runs at the end (warns on failure).

## `twig memo [TEXT] [--card <id>]`

> **In development.** First, simple version of [#16](https://github.com/RafaelKC/twig/issues/16).
> Behavior and file format may change before they are stable. Design and rationale:
> [MEMORY.md](MEMORY.md).

A small per-card memory that survives sessions and `twig rm`, so the next agent session (or a
reopened card) starts informed.

```bash
twig memo "api validation moved to the service layer; web just shows the error"
twig memo                      # prints the memo path and its content
twig memo -c 52644 "QA found X"   # another card, from anywhere
```

- **Line format:** appends one line, `YYYY-MM-DD HH:MM: TEXT` (local time), at the **end** of the
  memo. Newlines and runs of whitespace in TEXT collapse to single spaces. Empty TEXT is a usage
  error (exit 1). Several words without quotes are joined with spaces.
- **Without TEXT:** prints the memo file path and its content.
- **Which card:** inferred from the current directory (or the herdr plugin context) like
  `twig code`; outside a card it fails and asks for `--card`.
- **Where it lives:** the real file is `<worktrees>/memos/<id>-<provider>/MEMORY.md`
  (`<provider>` = `az` or `gh`, e.g. `memos/52644-az/MEMORY.md`). The card folder holds
  `MEMORY.md`, a **relative symlink** to it (`../memos/52644-az/MEMORY.md`). Directories and the
  file are created as needed. The `memos` folder is never treated as a card (`ls`, `status`,
  `sync`, `prune`, card inference ignore it).
- **`twig rm`:** deletes the card folder and the symlink, never the memo; it prints where the
  memo is kept.
- **`twig new <id>` on a card that has a memo:** recreates the symlink and says
  "memo restored". Use it when a closed card has to be reopened (e.g. QA asks).
- **Context file:** `new`, `add-repo` and `refresh` put the **last 10 entries** (constant
  `MEMO_CONTEXT_ENTRIES`) in the context file (`CLAUDE.md` / `AGENTS.md`) under
  `## Memory (last 10 entries, full file: MEMORY.md)`, before "Card context"; omitted when
  there is no memo.
- `new`, `add-repo` and `refresh` also make sure the symlink exists.

| Option | Description |
|---|---|
| `-c`, `--card <id>` | use this card instead of the inferred one; the card folder must exist, unless a memo for that id already exists |

**Limits and known follow-ups:** Unix symlinks only (if linking fails twig warns and prints the
real path); no consolidation of old entries; no size cap; no export on `rm` (needs lifecycle
hooks, [#15](https://github.com/RafaelKC/twig/issues/15)); no structured sections.

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

## `twig pr [id] [--draft] [-n | --dry-run]`

Opens the card's pull requests: one per repo, in a single command, through the provider's CLI
(`gh pr create` or `az repos pr create`, run inside the worktree so the repo is detected from
`origin`). Without `<id>`, the card of the current directory.

Per repo, in config order:

1. a detached HEAD is skipped;
2. a branch that already has an **open PR** just prints it (nothing is pushed or edited);
3. a branch with **no commits ahead** of `origin/<base>` is skipped (base: the card template's
   `base`, else `git.base_branch`);
4. otherwise the branch is pushed (`git push -u origin <branch>`) and the PR is opened into the
   base. Uncommitted changes only warn: they are not in the PR.

The PR title is the card title (the branch name when the card has no metadata). The description
links the card; on GitHub it also says `Closes owner/repo#N` (from a [repo-scoped id](#card-ids)
or the cached issue URL), so merging closes the issue; on Azure DevOps the work item is linked
with `--work-items`. When the card ends up with more than one PR, each PR opened in this run gets
an "Other PRs of this card" list with the sibling links (PRs that already existed are left as
they are).

| Option | Description |
|---|---|
| `--draft` | opens the PRs as drafts |
| `-n`, `--dry-run` | shows what would be pushed and opened, without doing it |

Exits 1 when the provider CLI is missing, or when a push or a PR creation failed (the other
repos still go on). Azure DevOps uses `provider.az.organization` when set, else what `az`
detects from the remote.

```bash
twig pr 52644 --draft
twig pr                      # current card
twig pr acme/web#12 -n
```

## `twig clean [-n] [-y] [--branches] [-f]`

Finds the **finished cards** and offers to remove them. A card is finished when:

- its work item / issue is closed in the provider (asked again, not read from the cache):
  Azure DevOps state `Closed`, `Done` or `Removed`; GitHub `CLOSED`; or
- **every** repo's branch got into the base (the card template's `base`, else
  `git.base_branch`): the provider has a merged PR for the branch (this catches squash merges),
  or the branch has commits of its own and all of them are in `origin/<base>`. A branch with no
  commits of its own (a card just created) never counts as merged.

Without the provider CLI, or offline, only the git check applies.

The candidates are listed with the reason; then each one is confirmed (`[y/N]`) unless `--yes`.
Removal is `twig rm` with the card's template active, so its safety checks hold: uncommitted
changes or a running agent block the card (the others go on; exit 1 at the end) unless `--force`,
and the `pre_rm`/`post_rm` hooks run.

| Option | Description |
|---|---|
| `-n`, `--dry-run` | only lists the finished cards |
| `-y`, `--yes` | doesn't ask |
| `--branches` | also deletes the local branches (as `rm --branches`) |
| `-f`, `--force` | removes even with uncommitted changes or a running agent |

`twig prune` is different: it only cleans orphaned worktree records and empty card folders.

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
| config | file exists and is valid; `TWIG__*` overrides in effect (info); each repo's `.twig.toml` in its main clone is valid | error |
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

## `twig template ls` / `show NAME` / `edit`

Card templates live in `templates.toml` next to the config (`TWIG_TEMPLATES` overrides the path);
format in [CONFIG.md](CONFIG.md#templates-and-templatestoml).

- `ls` prints each template and its repos (no file = a hint, exit 0).
- `show NAME` prints repos, agent, install, branch prefix/base and hooks; an unknown name lists the available ones.
- `edit` works like `config edit`: opens `$VISUAL`/`$EDITOR`, creates a commented file if missing and
  validates on save (repo names must exist in the config).

## `twig config edit`

Opens the config in `$VISUAL`, else `$EDITOR`, else `nano` (Windows: `notepad`). The value
may include arguments (e.g. `EDITOR="code --wait"`). Since it follows the symlink, it edits the
real file.

- Config doesn't exist → creates it from a minimal template before opening.
- When the editor closes, it validates. Invalid → shows the error and asks `reopen to fix it? [Y/n]`.
  Without a terminal, an empty answer counts as "no", to avoid looping.
- Exits with 1 if the file is left invalid.

## `twig config get <key>` / `twig config set <key> <value>`

Reads or writes one value by dotted key: `git.base_branch`, `open.default`,
`repos.my-backend.run`. Non-interactive, so it is the way for scripts and agents to change the
config (instead of editing the file by hand).

`get`:

- Scalars (string, number, bool) print bare, with no quotes. Arrays and tables print as TOML.
- Unknown key: error and exit 1.
- Only needs the file to be valid TOML, so it also works while a value is invalid.

`set`:

- Edits the file in place with comments, blank lines and key order preserved (the trailing
  comment of a replaced value stays). Missing tables (e.g. a new `[repos.x]`) are created.
- Validates the result with the same checks as `config edit`. Invalid (unknown key, wrong
  type, value not allowed) → error naming the key and the valid options, exit 1, **file
  untouched**.
- The file only has to be valid TOML: you can fix an invalid value with `set`, as long as the
  result is valid. The config must already exist (`twig config edit` creates it).
- Value types are inferred: `true`/`false` → bool, integers → number, anything else → string
  (quotes optional). If that reading is not valid for the field, it falls back to a string, then
  to a list (so `short 123` is the string `"123"` and `local_files .env` is `[".env"]`).
- Arrays: `a,b,c` or `[a, b, c]`; quoted TOML (`'["a b", "c"]'`) also works. `[]` empties it.
- Keys are split on `.`, so repo names containing a dot are not addressable (use `config edit`).

```bash
twig config set git.base_branch develop
twig config set open.default code
twig config set repos.my-backend.run "dotnet watch run"
twig config set repos.my-backend.local_files ".env,.env.local"
twig config get repos.my-backend
```

Not part of this command (still open on [#19](https://github.com/RafaelKC/twig/issues/19)):
per-repo `.twig.toml`, per-card overrides and environment variable overrides.

## `twig update [--check] [--version vX.Y.Z]`

Asks the GitHub API for the latest release (`TWIG_REPO`, default `RafaelKC/twig`; uses
`GITHUB_TOKEN` if set). If it's newer than the running version, downloads that tag's
`install.sh` and runs it with `--update`: twig, the AI skill, the herdr plugin and
completions are replaced; dependencies and config are not touched. Needs `curl`.

The skill is refreshed in every place a previous install put it (`~/.claude/skills/twig` for
Claude Code, `~/.agents/skills/twig` or `~/.codex/skills/twig` for Codex); `--update` never
creates a skill where there was none.

On Windows it downloads that tag's `install.ps1` instead and runs it with `-Update` through
`powershell -ExecutionPolicy Bypass -File`. The running `twig.exe` can't be overwritten, so the
installer renames it to `twig.exe.old` (removed on the next update) and puts the new one in
its place.

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
- shell completions: the `# twig completions` line and the `PATH` line the installer added to
  `~/.bashrc` / `~/.zshrc`, the fish file, and the static scripts older versions wrote;
- the `twig` binary on the `PATH`.

On Windows: the plugin folder is `%LOCALAPPDATA%\twig`, herdr's config `%APPDATA%\herdr\config.toml`,
the completion line is removed from the PowerShell profiles, the install folder from the user
`PATH`, and `twig.exe` (which is running) is moved to the temp folder instead of deleted.
`az` and `gh` are removed with `winget uninstall` when winget owns them.

Then asks (default no) whether to also remove the config and `az`, `gh` and `herdr`.
Tools are removed the way they were installed: Homebrew, apt, dnf or pacman when one of
them owns the package; otherwise the binary file, if it's under the home directory.
Card folders (`paths.worktrees`) are never touched; run `twig rm` first if you want.

| Option | Description |
|---|---|
| `-y`, `--yes` | don't ask: removes twig's pieces, keeps the config and the dependencies |

Without a terminal and without `--yes`, it cancels.

## `twig completions <shell>`

Prints the completion hook for `bash`, `zsh`, `fish`, `powershell` or `elvish`. The hook is a
few lines that call back into twig (`COMPLETE=<shell> twig -- <words>`), so the completions are
answered by the installed binary: they never go stale after `twig update`, and besides commands
and options they complete **values**: card ids (with the card title in shells that show
descriptions) for every command that takes one, repos for `--repos` / `add-repo`, templates for
`--template` / `template show`, targets for `open`. Values never query the provider.

The installer enables it on its own (also on `twig update`, replacing the large static script
older versions wrote):

| Shell | Where |
|---|---|
| bash | `~/.bashrc`: `command -v twig >/dev/null 2>&1 && source <(COMPLETE=bash twig)  # twig completions` |
| zsh | `~/.zshrc`: the same with `COMPLETE=zsh`, guarded by `compdef` (it must come after `compinit`, which zsh frameworks load earlier in the file) |
| fish | `~/.config/fish/completions/twig.fish`: `COMPLETE=fish twig \| source` |

`twig uninstall` removes those lines and files. By hand: `source <(twig completions bash)`.

## `twig help [command]`

General help, or detailed help for one command. Every command also accepts `-h` / `--help`.

## Outside the CLI

| Item | What it does |
|---|---|
| Completions | commands, options, card ids, repos and templates (see [`twig completions`](#twig-completions-shell)) |
| herdr plugin | `open-code` action (`prefix+shift+o`): opens the focused card in VS Code; `launcher.sh` popup (`prefix+shift+s`): runs a twig/shell command |
| Claude Code skill | `skills/twig/`: teaches the agent the card workflow and custom herdr layouts |

## Open questions

- `--no-ado` with the `gh` provider: rename to something generic (e.g. `--offline`).
- `--fix`: generalize to `--prefix NAME`, since prefixes come from the config.
