# herdr: custom workspaces, tabs, and panes

herdr organizes terminals into **workspaces → tabs → panes** and is controlled by
the `herdr` CLI, which talks to the server over a socket. Almost every command returns
JSON: **read the IDs from the response**, never guess them.

> The installed binary is the authority. When in doubt, `herdr <group> <command> --help`
> (e.g. `herdr pane split --help`). **Never run bare `herdr`** to find something
> out: it opens the UI and blocks the terminal. Nor run a create command
> without arguments to "see what it does": `herdr workspace create` works with the
> defaults and really creates one.

## Before controlling herdr

```bash
test "${HERDR_ENV:-}" = 1 && echo inside || echo outside
```

- **Inside** (running in a herdr pane): you may control it. `HERDR_WORKSPACE_ID`,
  `HERDR_TAB_ID`, and `HERDR_PANE_ID` tell you where you are.
- **Outside**: only touch herdr if the user explicitly asked, and never run
  `herdr` without a subcommand. If the server isn't up, commands fail;
  `twig herdr <id>` starts the server on its own.

## IDs

| Object | Format | Comes from |
|---|---|---|
| workspace | `w1` | `.result.workspace.workspace_id` |
| tab | `w1:t1` | `.result.tab.tab_id` |
| pane | `w1:p1` | `.result.root_pane.pane_id` or `.result.pane.pane_id` |

IDs of closed tabs and panes are not reused. A twig card's workspace has
**label = card id** (e.g. `52644`), but its ID is different (e.g. `w13`).

## Inspecting state

```bash
herdr workspace list                              # .result.workspaces[] {workspace_id, label, focused}
herdr tab list --workspace <ws>                   # .result.tabs[] {tab_id, label}
herdr pane list --workspace <ws>                  # .result.panes[] {pane_id, tab_id, cwd, foreground_cwd}
herdr pane current --current                      # the pane you are in
herdr pane layout --pane "$HERDR_PANE_ID"         # geometry, to decide the split direction
```

Finding a card's workspace and a repo's pane (herdr doesn't expose pane labels in
the listing; twig finds the pane by its `cwd`):

```bash
ws=$(herdr workspace list | jq -r '.result.workspaces[] | select(.label=="52644") | .workspace_id')
tab=$(herdr tab list --workspace "$ws" | jq -r '.result.tabs[] | select(.label=="run") | .tab_id')
pane=$(herdr pane list --workspace "$ws" | jq -r --arg t "$tab" --arg c "$HOME/.wt/52644/my-backend" \
  '.result.panes[] | select(.tab_id==$t and (.cwd==$c or .foreground_cwd==$c)) | .pane_id')
```

Without `jq`, use `python3 -c 'import json,sys; ...'` on the output.

## Creating

### Workspace

```bash
herdr workspace create --cwd <dir> --label <name> --no-focus
# → .result.workspace.workspace_id, .result.tab.tab_id, .result.root_pane.pane_id
herdr tab rename <tab_id> <label>                # the initial tab has no useful name
```

### Tab

```bash
herdr tab create --workspace <ws> --cwd <dir> --label <label> --no-focus
# → .result.tab.tab_id, .result.root_pane.pane_id
```

### Pane (split)

```bash
herdr pane split <pane_id> --direction right --cwd <dir> --no-focus
# → .result.pane.pane_id
```

- `--direction right` (side by side) or `down` (stacked). Split a wide pane to the
  right and a narrow one down; avoid many splits in the same direction.
- `--ratio 0.3` controls the size; `--env KEY=value` passes a variable to the process.
- `--current` instead of the ID splits the pane you are in.
- **Always use `--no-focus`** so you don't steal the user's focus, unless they ask.

### Naming

```bash
herdr pane rename <pane_id> <label>
herdr pane rename <pane_id> --clear
herdr workspace rename <ws> <label>
```

## Running commands and reading output

```bash
herdr pane run <pane_id> "pnpm test"                               # text + Enter in one go
herdr pane wait-output <pane_id> --match "Tests:" --timeout 120000 # or --regex '<rust regex>'
herdr pane read <pane_id> --source recent-unwrapped --lines 120    # logs: prefer recent-unwrapped
herdr pane send-keys <pane_id> ctrl+c                               # keys (esc, enter, ctrl+c...)
herdr pane send-text <pane_id> "text without enter"
```

`wait-output` also searches output that already exists; without `--timeout`, it waits
forever. A timeout doesn't prove the command didn't run: read the pane before retrying.

## Focusing and closing

```bash
herdr workspace focus <ws>
herdr tab focus <tab_id>
herdr pane close <pane_id>
herdr tab close <tab_id>
herdr workspace close <ws>     # twig rm does NOT close the herdr workspace; close it if the user asks
```

Closing kills the pane's processes. Confirm with the user before closing anything
you didn't create.

## Recipes

### Extra tab in a card (e.g. back-end tests in watch mode)

```bash
ws=$(herdr workspace list | jq -r '.result.workspaces[] | select(.label=="52644") | .workspace_id')
pane=$(herdr tab create --workspace "$ws" --cwd "$HOME/.wt/52644/my-backend" --label test --no-focus \
  | jq -r '.result.root_pane.pane_id')
herdr pane rename "$pane" back
herdr pane run "$pane" "dotnet watch test"
```

### Log pane next to the current pane

```bash
pane=$(herdr pane split --current --direction right --cwd "$PWD" --no-focus | jq -r '.result.pane.pane_id')
herdr pane run "$pane" "tail -f logs/app.log"
```

### Standalone workspace with its own layout (2 columns, the right one split)

```bash
json=$(herdr workspace create --cwd ~/project --label sandbox --no-focus)
ws=$(jq -r '.result.workspace.workspace_id' <<<"$json")
left=$(jq -r '.result.root_pane.pane_id' <<<"$json")
herdr tab rename "$(jq -r '.result.tab.tab_id' <<<"$json")" main
right=$(herdr pane split "$left" --direction right --cwd ~/project --no-focus | jq -r '.result.pane.pane_id')
bottom=$(herdr pane split "$right" --direction down --cwd ~/project --no-focus | jq -r '.result.pane.pane_id')
herdr pane run "$left" "claude"
herdr pane run "$right" "pnpm start"
herdr pane run "$bottom" "git log --oneline -20"
```

### Another agent in a pane

`herdr pane run <pane> claude` works to start the agent. To talk to it
(prompt, wait for it to finish, read the reply), use the `herdr agent` group
(`herdr agent --help`): `agent start`, `agent prompt ... --wait`, `agent read`. Only do
this if the user asks for another agent; don't create layouts on your own.

## twig conventions in herdr

- Card workspace: label = `<id>` (the card folder name: `52644`, or `repo-12` for a GitHub
  `owner/repo#12` card); tabs `agent` and `run`; each pane named after the repo's `short`; each
  repo's pane is found by its `cwd` (`~/.wt/<id>/<repo>`).
- Panes twig creates carry the card's ports and the repo's `env` (`herdr ... --env`). A pane you
  create yourself doesn't: pass them with `--env` (`twig ports <id> --env` lists them) or run
  `eval "$(twig env)"` inside it (ports, `PORT` and the repo's `env`; also fixes panes herdr restored after a reboot).
- Pane layout: `[herdr.layout]` (`agent`/`run` = `columns|rows|main-left|main-top|grid`, `order`) in
  the config, a template or the card's `.twig.toml`; `main-left` is the default. `twig layout [id]
  [--tab agent|run] <preset>` rearranges an existing tab keeping its panes and processes. Don't
  rebuild a card's layout by hand with `herdr pane move`: it can't move within the same tab.
- Extra panes and tabs you create don't get in twig's way: `twig add-repo` and
  `twig herdr` only create what's missing from the default layout and leave the rest alone.
- `twig park` saves the workspace (tabs, splits, panes, agent session ids) in
  `<card>/.twig-park-layout.json` and `twig open` rebuilds exactly that, without recreating the
  default panes the user closed; `twig open --default-layout` ignores it.
- Don't reuse the `agent`/`run` tabs for something else; create a new tab.
