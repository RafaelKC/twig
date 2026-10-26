# herdr plugins

herdr plugins that ship with twig. Each one lives in its own folder with a
`herdr-plugin.toml`. A third-party plugin is only adopted after its manifest and
code are reviewed, with the commit pinned; candidates are in
[refs/herdr-plugins.md](../refs/herdr-plugins.md).

| Plugin | Folder | Action | Suggested shortcut | What it does |
|---|---|---|---|---|
| `twig` | [twig/](twig/) | `twig.open-code` | `prefix+shift+o` | opens the focused workspace's card in VS Code |
| `twig` | [twig/](twig/) | `launcher.sh` (popup, no action) | `prefix+shift+s` | popup that runs a twig command (or shell command) in the foreground |

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

### Launcher

`prefix+shift+s` opens a herdr popup (`type = "popup"`, herdr ≥ 0.9; 70% × 60%) running
[`launcher.sh`](twig/launcher.sh). It doesn't touch tabs or panes:

1. type a command; the `twig` prefix is implicit (`new 123`, `ls`, `rm 123`). If the first word
   isn't a twig subcommand (or alias), the line runs as a shell command;
2. it runs in the foreground with the full output, and twig's interactive prompts work (the
   popup is a real terminal);
3. it shows `[exit N]` and `[enter] fechar / close`; the popup closes only after enter.
   An empty line or Esc + enter closes without running anything.

It finds `twig` like `open-code.sh` and drops the card env (`TWIG_*` except the config
overrides, and `PORT`) in case the popup was opened from a card pane. Unix only for now (no
`launcher.ps1`).

### Install

[`install.sh`](../install.sh) installs it for you: it copies the plugin to
`~/.local/share/twig/herdr-plugins/twig`, links it, and adds the shortcut below between
`# twig:begin` / `# twig:end` (each one skipped if its key is already bound).
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

```toml
[[keys.command]]
key = "prefix+shift+s"
type = "popup"
command = "sh ~/.local/share/twig/herdr-plugins/twig/launcher.sh"
width = "70%"
height = "60%"
description = "twig launcher"
```

Then `herdr server reload-config`.

Logs: `herdr plugin log list --plugin twig`.
