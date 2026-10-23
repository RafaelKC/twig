# herdr plugins

herdr plugins that ship with twig. Each one lives in its own folder with a
`herdr-plugin.toml`. A third-party plugin is only adopted after its manifest and
code are reviewed, with the commit pinned; candidates are in
[refs/herdr-plugins.md](../refs/herdr-plugins.md).

| Plugin | Folder | Action | Suggested shortcut | What it does |
|---|---|---|---|---|
| `twig` | [twig/](twig/) | `twig.open-code` | `prefix+shift+o` | opens the focused workspace's card in VS Code |

## `twig`

`open-code` runs `twig code` with no id, so twig works out the card from the
context herdr passes to the plugin (`HERDR_PLUGIN_CONTEXT_JSON`):

1. workspace label is a card id (twig names card workspaces `<id>`) → opens that card;
2. else the focused pane's cwd (or the workspace's) is inside `<worktrees>/<id>/` → that card;
3. else opens that folder in VS Code.

If the card has no `.code-workspace` yet, `twig code` generates it. It errors if
`vscode.enabled = false` or `code` isn't found: twig looks on the PATH, then in the usual
install spots (`/usr/local/bin`, `/usr/bin`, `/snap/bin`, `/opt/homebrew/bin`, `~/.local/bin`, the macOS app bundle), since the herdr server's PATH is often shorter than your shell's.

Requirements: `twig` on the herdr server's PATH, or at `~/.local/bin/twig` /
`~/.cargo/bin/twig`.

### Install

[`install.sh`](../install.sh) installs it for you: it copies the plugin to
`~/.local/share/twig/herdr-plugins/twig`, links it, and adds the shortcut below between
`# twig:begin` / `# twig:end` (skipped if `prefix+shift+o` is already bound).
`twig uninstall` removes both.

By hand, from a clone:

```bash
herdr plugin link "$PWD/herdr-plugins/twig"
```

Shortcut in `~/.config/herdr/config.toml`:

```toml
[[keys.command]]
key = "prefix+shift+o"
type = "plugin_action"
command = "twig.open-code"
description = "open card in VS Code"
```

Then `herdr server reload-config`.

Logs: `herdr plugin log list --plugin twig`.
