#!/bin/sh
# Opens the focused workspace's card in VS Code. `twig code` without an id reads the
# card from HERDR_PLUGIN_CONTEXT_JSON (workspace label, else pane cwd); outside a card
# it opens that folder. It also strips HERDR_* so VS Code's terminal can run herdr.
set -eu

# the herdr server's PATH may not include ~/.local/bin or ~/.cargo/bin
twig=$(command -v twig || true)
for candidate in "$HOME/.local/bin/twig" "$HOME/.cargo/bin/twig"; do
  [ -n "$twig" ] && break
  [ -x "$candidate" ] && twig=$candidate
done

if [ -z "$twig" ]; then
  echo "twig not found in the herdr server's PATH (nor in ~/.local/bin or ~/.cargo/bin)" >&2
  exit 1
fi

exec "$twig" code
