#!/bin/sh
# herdr popup launcher: asks for one line, runs it in the foreground with the full output
# and the real tty (twig's prompts read /dev/tty), then waits for enter before closing.
# `new 123` runs `twig new 123`; if the first word isn't a twig subcommand, the line runs
# as a plain shell command. Empty line or Esc closes without running anything.
set -u

# the herdr server's PATH may not include ~/.local/bin or ~/.cargo/bin
twig=$(command -v twig || true)
for candidate in "$HOME/.local/bin/twig" "$HOME/.cargo/bin/twig"; do
  [ -n "$twig" ] && break
  [ -x "$candidate" ] && twig=$candidate
done

# the popup must not inherit the env of a card pane it was opened from
for name in $(env | sed -n 's/^\(TWIG_[A-Za-z0-9_]*\)=.*/\1/p'); do
  case "$name" in
    TWIG_CONFIG | TWIG_TEMPLATES | TWIG_REPO | TWIG_NO_UPDATE_CHECK) ;;
    *) unset "$name" ;;
  esac
done
unset PORT

confirm_close() {
  printf '\n[enter] fechar / close '
  read -r _ || true
}

printf 'twig> '
IFS= read -r line || exit 0
esc=$(printf '\033')
case "$line" in "$esc"*) exit 0 ;; esac
# trim blanks
line=${line#"${line%%[![:space:]]*}"}
line=${line%"${line##*[![:space:]]}"}
[ -z "$line" ] && exit 0

# Ctrl-C stops the command, not the launcher (the exit code and confirmation still show)
trap ':' INT

first=${line%%[[:space:]]*}
is_twig=0
if [ -n "$twig" ]; then
  case "$first" in
    -*) is_twig=1 ;;
    *)
      # subcommands and their aliases, from the `Commands:` section of `twig --help`
      subs=$("$twig" --help 2>/dev/null | awk '
        /^Commands:/ { f = 1; next }
        /^[^ ]/ { f = 0 }
        f && NF { print $1; if (match($0, /\[alias: [^]]*\]/)) print substr($0, RSTART + 8, RLENGTH - 9) }')
      for s in $subs; do
        [ "$s" = "$first" ] && is_twig=1
      done
      ;;
  esac
fi

echo
if [ "$is_twig" = 1 ]; then
  eval "\"\$twig\" $line"
  code=$?
elif [ -z "$twig" ] && [ "$first" = "twig" ]; then
  echo "twig not found in the herdr server's PATH (nor in ~/.local/bin or ~/.cargo/bin)" >&2
  code=127
else
  sh -c "$line"
  code=$?
fi

printf '\n[exit %s]' "$code"
confirm_close
exit 0
