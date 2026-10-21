#!/bin/sh
# twig installer.
#
#   curl -fsSL https://rndm.chicovis.dev/twig/unix-install | sh
#
# Checks/installs the dependencies (docs/DEPENDENCIES.md), downloads the twig release
# for this OS/arch, verifies its checksum (and Sigstore signature when cosign is
# available), and installs the binary, the AI skill, the herdr plugin and shell
# completions. Running it again updates an existing install.
#
# Options:
#   --version vX.Y.Z   install that release (default: latest)
#   --update           only update twig, skill, plugin and completions (no dependency setup)
#   --bin-dir DIR      where to put the binary (default: existing install, else ~/.local/bin)
#   -y, --yes          accept defaults without asking
#
# Env: TWIG_REPO (owner/repo), TWIG_VERSION, TWIG_BIN_DIR, TWIG_CONFIG,
#      TWIG_DIST_DIR (use local release files instead of downloading; for testing).

set -eu

REPO="${TWIG_REPO:-RafaelKC/twig}"
VERSION="${TWIG_VERSION:-}"
BIN_DIR="${TWIG_BIN_DIR:-}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/twig"
CONFIG_FILE="${TWIG_CONFIG:-$HOME/.config/twig/config.toml}"
HERDR_CONFIG="${HERDR_CONFIG_PATH:-$HOME/.config/herdr/config.toml}"
MODE=install
YES=0

# ───────────────────────── output and prompts ─────────────────────────

if [ -t 2 ]; then
  C_INFO='\033[36m' C_OK='\033[32m' C_WARN='\033[33m' C_ERR='\033[31m' C_BOLD='\033[1m' C_OFF='\033[0m'
else
  C_INFO='' C_OK='' C_WARN='' C_ERR='' C_BOLD='' C_OFF=''
fi

info() { printf "${C_INFO}→ %s${C_OFF}\n" "$*" >&2; }
ok() { printf "${C_OK}✔ %s${C_OFF}\n" "$*" >&2; }
warn() { printf "${C_WARN}! %s${C_OFF}\n" "$*" >&2; }
die() { printf "${C_ERR}✖ %s${C_OFF}\n" "$*" >&2; exit 1; }
section() { printf "\n${C_BOLD}%s${C_OFF}\n" "$*" >&2; }

have() { command -v "$1" >/dev/null 2>&1; }

# A controlling terminal is still reachable when the script itself comes through a pipe.
has_tty() { [ "$YES" = 0 ] && (exec </dev/tty) 2>/dev/null; }

# ask "prompt" "default" → answer on stdout (the default without a terminal or with --yes)
ask() {
  if has_tty; then
    printf "%s " "$1" >&2
    read -r answer </dev/tty || answer=""
    [ -n "$answer" ] && { printf '%s' "$answer"; return; }
  fi
  printf '%s' "$2"
}

# confirm "question" y|n → exit status
confirm() {
  if [ "$2" = y ]; then hint="[Y/n]"; else hint="[y/N]"; fi
  case "$(ask "$1 $hint" "$2")" in
    y | Y | s | S | yes | sim) return 0 ;;
    *) return 1 ;;
  esac
}

# run an interactive command on the terminal (logins, installers that prompt)
run_tty() {
  if has_tty; then "$@" </dev/tty; else "$@"; fi
}

# ───────────────────────── arguments ─────────────────────────

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --update) MODE=update; shift ;;
    --bin-dir) BIN_DIR="$2"; shift 2 ;;
    -y | --yes) YES=1; shift ;;
    -h | --help) sed -n '2,20p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

# ───────────────────────── system detection ─────────────────────────

case "$(uname -s)" in
  Linux) OS=linux ;;
  Darwin) OS=macos ;;
  *) die "unsupported system: $(uname -s) (Linux and macOS only)" ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) die "unsupported architecture: $(uname -m)" ;;
esac

if [ "$OS" = linux ]; then TARGET="$ARCH-unknown-linux-musl"; else TARGET="$ARCH-apple-darwin"; fi

IS_WSL=0
[ "$OS" = linux ] && grep -qi microsoft /proc/version 2>/dev/null && IS_WSL=1

PM=""
for candidate in brew apt-get dnf pacman zypper; do
  if have "$candidate"; then PM=$candidate; break; fi
done

SUDO=""
if [ "$(id -u)" != 0 ] && have sudo; then SUDO=sudo; fi

have curl || die "curl is required to install twig"
have tar || die "tar is required to install twig"

# ───────────────────────── package installs ─────────────────────────

pm_install() { # pm_install <brew> <apt> <dnf> <pacman> <zypper>
  case "$PM" in
    brew) brew install "$1" ;;
    apt-get) $SUDO apt-get update -qq && $SUDO apt-get install -y "$2" ;;
    dnf) $SUDO dnf install -y "$3" ;;
    pacman) $SUDO pacman -S --noconfirm "$4" ;;
    zypper) $SUDO zypper install -y "$5" ;;
    *) return 1 ;;
  esac
}

install_git() {
  if [ "$OS" = macos ] && [ "$PM" != brew ]; then
    xcode-select --install || true
    die "install the Xcode Command Line Tools (a window just opened) and run the installer again"
  fi
  pm_install git git git git git || die "don't know how to install git here; install it manually"
}

install_herdr() {
  if [ "$PM" = brew ]; then
    brew install herdr
  else
    curl -fsSL https://herdr.dev/install.sh | sh
  fi
  # the herdr installer may use a dir that isn't on this shell's PATH yet
  have herdr || PATH="$HOME/.local/bin:$PATH"
}

install_gh() {
  case "$PM" in
    brew | dnf | zypper) pm_install gh gh gh gh gh ;;
    pacman) pm_install gh gh gh github-cli gh ;;
    apt-get)
      # official GitHub CLI apt repository (cli.github.com)
      $SUDO mkdir -p -m 755 /etc/apt/keyrings
      curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg |
        $SUDO tee /etc/apt/keyrings/githubcli-archive-keyring.gpg >/dev/null
      $SUDO chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg
      echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" |
        $SUDO tee /etc/apt/sources.list.d/github-cli.list >/dev/null
      $SUDO apt-get update -qq && $SUDO apt-get install -y gh
      ;;
    *) die "don't know how to install gh here; see https://cli.github.com" ;;
  esac
}

install_az() {
  case "$PM" in
    brew) brew install azure-cli ;;
    apt-get) curl -fsSL https://aka.ms/InstallAzureCLIDeb | $SUDO bash ;;
    *) die "don't know how to install az here; see https://learn.microsoft.com/cli/azure/install-azure-cli" ;;
  esac
}

install_vscode() {
  if [ "$IS_WSL" = 1 ]; then
    warn "on WSL, install VS Code on Windows (https://code.visualstudio.com) with the WSL extension; \`code\` then shows up on its own"
    return 1
  fi
  if [ "$PM" = brew ]; then
    brew install --cask visual-studio-code
  elif have snap; then
    $SUDO snap install code --classic
  else
    warn "install VS Code manually: https://code.visualstudio.com/download"
    return 1
  fi
}

install_claude() {
  curl -fsSL https://claude.ai/install.sh | bash
  have claude || PATH="$HOME/.local/bin:$PATH"
}

install_codex() {
  if have npm; then
    npm install -g @openai/codex
  elif have brew; then
    brew install codex
  else
    warn "install Codex manually: https://github.com/openai/codex"
  fi
}

# ───────────────────────── dependencies ─────────────────────────

AI=""
VSCODE_ENABLED=false
PROVIDER=""
AZ_ORG=""

setup_git() {
  section "git"
  if have git; then
    ok "git $(git --version | awk '{print $3}')"
  elif confirm "git not found (required). Install it?" y; then
    install_git
    ok "git installed"
  else
    die "git is required"
  fi

  name=$(git config --global user.name || true)
  email=$(git config --global user.email || true)
  if [ -n "$name" ] && [ -n "$email" ]; then
    ok "git identity: $name <$email>"
  elif confirm "git user.name/user.email not set. Set them now?" y; then
    [ -n "$name" ] || name=$(ask "name:" "")
    [ -n "$email" ] || email=$(ask "email:" "")
    [ -n "$name" ] && git config --global user.name "$name"
    [ -n "$email" ] && git config --global user.email "$email"
    if [ -n "$name" ] && [ -n "$email" ]; then
      ok "git identity set"
    else
      warn "git identity incomplete (git config --global user.name/user.email)"
    fi
  else
    warn "git identity not set (\`twig doctor\` will keep warning about it)"
  fi
}

setup_herdr() {
  section "herdr"
  if have herdr; then
    ok "$(herdr --version 2>/dev/null || echo herdr)"
  elif confirm "herdr not found (required). Install it?" y; then
    install_herdr
    have herdr || die "herdr was installed but is not on PATH; open a new terminal and run the installer again"
    ok "herdr installed"
  else
    die "herdr is required"
  fi
}

setup_vscode() {
  section "VS Code (optional)"
  if have code; then
    ok "code found"
    VSCODE_ENABLED=true
  elif confirm "VS Code not found. Install it?" n && install_vscode && have code; then
    ok "VS Code installed"
    VSCODE_ENABLED=true
  else
    info "continuing without VS Code (vscode.enabled = false)"
  fi
}

check_az_login() {
  if ! az extension show --name azure-devops >/dev/null 2>&1; then
    info "adding the az azure-devops extension"
    az extension add --name azure-devops --only-show-errors || warn "could not add the azure-devops extension"
  fi
  AZ_ORG=$(az devops configure --list 2>/dev/null | sed -n 's/^organization *= *//p')
  if [ -z "$AZ_ORG" ]; then
    AZ_ORG=$(ask "Azure DevOps organization (e.g. https://dev.azure.com/my-org, empty to skip):" "")
    [ -n "$AZ_ORG" ] && az devops configure --defaults organization="$AZ_ORG"
  fi
  if az devops project list --top 1 --output none >/dev/null 2>&1; then
    ok "az has access to Azure DevOps"
  elif confirm "az has no access to Azure DevOps. Log in now?" y; then
    run_tty az login || warn "az login failed"
  else
    warn "az not logged in (az login or az devops login)"
  fi
}

check_gh_login() {
  if gh auth status >/dev/null 2>&1; then
    ok "gh logged in"
  elif confirm "gh is not logged in. Log in now?" y; then
    run_tty gh auth login || warn "gh login failed"
  else
    warn "gh not logged in (gh auth login)"
  fi
}

setup_provider() {
  section "card provider (az and/or gh)"
  chosen=""
  for p in az gh; do
    if have "$p" && confirm "$p is already installed. Use it?" y; then chosen="$chosen $p"; fi
  done
  if [ -z "$chosen" ]; then
    case "$(ask "choose a provider: [1] az (Azure DevOps) [2] gh (GitHub)" 1)" in
      2 | gh) install_gh; chosen=" gh" ;;
      *) install_az; chosen=" az" ;;
    esac
  fi
  ok "provider:$chosen"
  for p in $chosen; do
    [ -z "$PROVIDER" ] && PROVIDER=$p
    case "$p" in
      az) check_az_login ;;
      gh) check_gh_login ;;
    esac
  done
}

setup_ai() {
  section "AI"
  if have claude && have codex; then
    echo "  Claude Code and Codex are both installed."
  fi
  choice=$(ask "which agent do you want to use? (claude/codex) [claude]:" "claude")
  case "$choice" in
    codex)
      if have codex; then
        AI=codex && ok "using Codex"
      elif confirm "Codex not found. Install it?" y; then
        install_codex
        have codex && AI=codex && ok "Codex installed"
      fi
      ;;
    *)
      if have claude; then
        if confirm "Claude Code is already installed. Use it?" y; then AI=claude && ok "using Claude Code"; fi
      elif confirm "Claude Code not found. Install it?" y; then
        install_claude
        have claude && AI=claude && ok "Claude Code installed"
      fi
      ;;
  esac
  [ -n "$AI" ] || warn "no AI: the herdr agent tab will have nothing to run"
}

# ───────────────────────── download and verify ─────────────────────────

sha256_check() { # sha256_check <file> <expected>
  if have sha256sum; then
    actual=$(sha256sum "$1" | awk '{print $1}')
  else
    actual=$(shasum -a 256 "$1" | awk '{print $1}')
  fi
  [ "$actual" = "$2" ]
}

download_release() {
  section "twig"
  if [ -z "$VERSION" ]; then
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
      sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
    [ -n "$VERSION" ] || die "could not find the latest release of $REPO"
  fi
  asset="twig-$TARGET.tar.gz"
  base="https://github.com/$REPO/releases/download/$VERSION"
  TMP=$(mktemp -d)
  trap 'rm -rf "$TMP"' EXIT

  info "downloading twig $VERSION ($TARGET)"
  for f in "$asset" SHA256SUMS SHA256SUMS.sigstore.json; do
    if [ -n "${TWIG_DIST_DIR:-}" ]; then
      [ -f "$TWIG_DIST_DIR/$f" ] && cp "$TWIG_DIST_DIR/$f" "$TMP/$f"
    else
      curl -fsSL -o "$TMP/$f" "$base/$f" || [ "$f" = SHA256SUMS.sigstore.json ] || die "failed to download $base/$f"
    fi
  done
  [ -f "$TMP/$asset" ] || die "release $VERSION has no $asset"

  expected=$(awk -v a="$asset" '$2 == a || $2 == "*"a {print $1}' "$TMP/SHA256SUMS")
  [ -n "$expected" ] || die "$asset is not in SHA256SUMS"
  sha256_check "$TMP/$asset" "$expected" || die "checksum mismatch for $asset"
  ok "checksum verified"

  if [ -f "$TMP/SHA256SUMS.sigstore.json" ] && have cosign; then
    cosign verify-blob \
      --bundle "$TMP/SHA256SUMS.sigstore.json" \
      --certificate-identity-regexp "^https://github.com/$REPO/\.github/workflows/release\.yml@refs/tags/" \
      --certificate-oidc-issuer https://token.actions.githubusercontent.com \
      "$TMP/SHA256SUMS" >/dev/null 2>&1 || die "invalid Sigstore signature for $VERSION"
    ok "Sigstore signature verified"
  else
    info "cosign not installed: signature not verified (the checksum was). Manual verification in docs/RELEASING.md"
  fi

  tar -xzf "$TMP/$asset" -C "$TMP"
  PKG="$TMP/twig-$TARGET"
  [ -x "$PKG/twig" ] || die "package is missing the twig binary"
}

# ───────────────────────── install pieces ─────────────────────────

install_binary() {
  existing=$(command -v twig 2>/dev/null || true)
  OLD_VERSION=""
  [ -n "$existing" ] && OLD_VERSION=$("$existing" --version 2>/dev/null | awk '{print $2}' || true)
  if [ -z "$BIN_DIR" ]; then
    if [ -n "$existing" ] && [ ! -L "$existing" ] && [ -w "$(dirname "$existing")" ]; then
      BIN_DIR=$(dirname "$existing")
    else
      BIN_DIR="$HOME/.local/bin"
    fi
  fi
  mkdir -p "$BIN_DIR"
  rm -f "$BIN_DIR/twig" # never write through an old symlink
  cp "$PKG/twig" "$BIN_DIR/twig"
  chmod 755 "$BIN_DIR/twig"
  TWIG="$BIN_DIR/twig"
  if [ -n "$OLD_VERSION" ]; then
    ok "twig $OLD_VERSION → ${VERSION#v} at $TWIG"
  else
    ok "twig ${VERSION#v} installed at $TWIG"
  fi

  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
      rc=""
      case "$(basename "${SHELL:-sh}")" in
        bash) rc="$HOME/.bashrc" ;;
        zsh) rc="$HOME/.zshrc" ;;
      esac
      if [ -n "$rc" ] && ! grep -qs "$BIN_DIR" "$rc" && confirm "$BIN_DIR is not on PATH. Add it to $rc?" y; then
        # shellcheck disable=SC2016 # $PATH must expand when the rc file runs, not now
        printf '\nexport PATH="%s:$PATH"  # twig\n' "$BIN_DIR" >>"$rc"
        ok "PATH updated in $rc (open a new terminal)"
      else
        warn "add $BIN_DIR to your PATH"
      fi
      ;;
  esac
}

copy_skill() {
  mkdir -p "$(dirname "$1")"
  rm -rf "$1"
  cp -R "$PKG/skills/twig" "$1"
  ok "$2 skill at $1"
}

install_skill() {
  # --update doesn't ask which agent is used: refresh every copy an earlier install left
  if [ "$MODE" = update ]; then
    for spec in "$HOME/.claude/skills/twig:Claude Code" "$HOME/.agents/skills/twig:Codex" "$HOME/.codex/skills/twig:Codex"; do
      dest="${spec%%:*}"
      if [ -e "$dest" ]; then
        copy_skill "$dest" "${spec#*:}"
      fi
    done
    return 0
  fi
  case "$AI" in
    codex) dest="$HOME/.agents/skills/twig"; name="Codex" ;;
    *) dest="$HOME/.claude/skills/twig"; name="Claude Code" ;;
  esac
  if [ -z "$AI" ] && [ ! -e "$dest" ]; then return; fi
  copy_skill "$dest" "$name"
}

install_herdr_plugin() {
  have herdr || { warn "herdr not found: plugin not installed"; return; }
  pdir="$DATA_DIR/herdr-plugins/twig"
  mkdir -p "$DATA_DIR/herdr-plugins"
  rm -rf "$pdir"
  cp -R "$PKG/herdr-plugins/twig" "$pdir"
  herdr plugin unlink twig >/dev/null 2>&1 || true
  if herdr plugin link "$pdir" >/dev/null 2>&1; then
    ok "herdr plugin linked ($pdir)"
  else
    warn "could not link the plugin; run: herdr plugin link $pdir"
    return
  fi

  if grep -qs '# twig:begin' "$HERDR_CONFIG"; then
    : # shortcut already added by a previous install
  elif grep -qs 'key = "prefix+shift+o"' "$HERDR_CONFIG"; then
    warn "prefix+shift+o is already bound to another herdr shortcut; point it to \"twig.open-code\" in $HERDR_CONFIG if you want"
  else
    mkdir -p "$(dirname "$HERDR_CONFIG")"
    cat >>"$HERDR_CONFIG" <<'EOF'

# twig:begin (managed by the twig installer; `twig uninstall` removes it)
[[keys.command]]
key = "prefix+shift+o"
type = "plugin_action"
command = "twig.open-code"
description = "open card in VS Code"
# twig:end
EOF
    ok "shortcut prefix+shift+o → twig.open-code"
  fi
  herdr server reload-config >/dev/null 2>&1 || true
}

install_completions() {
  shell=$(basename "${SHELL:-sh}")
  case "$shell" in
    bash) file="${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions/twig" ;;
    zsh) file="${XDG_DATA_HOME:-$HOME/.local/share}/zsh/site-functions/_twig" ;;
    fish) file="$HOME/.config/fish/completions/twig.fish" ;;
    *) return ;;
  esac
  mkdir -p "$(dirname "$file")"
  "$TWIG" completions "$shell" >"$file" && ok "completions ($shell) at $file"
  if [ "$shell" = zsh ]; then
    info "on zsh, make sure ~/.zshrc has: fpath=(${file%/*} \$fpath)"
  fi
}

write_config() {
  [ -f "$CONFIG_FILE" ] && { ok "config kept: $CONFIG_FILE"; return; }
  section "config"
  # shellcheck disable=SC2088 # literal ~ on purpose: twig expands it when reading the config
  repos=$(ask "folder where your repo clones live [~]:" "~")
  # shellcheck disable=SC2088
  worktrees=$(ask "folder for card worktrees [~/.wt]:" "~/.wt")
  mkdir -p "$(dirname "$CONFIG_FILE")"
  cat >"$CONFIG_FILE" <<EOF
# twig config. Format: https://github.com/$REPO/blob/main/docs/CONFIG.md
version = 1

[agent]
provider = "${AI:-claude}"

[provider]
default = "${PROVIDER:-az}"

[provider.az]
organization = "$AZ_ORG"

[vscode]
enabled     = $VSCODE_ENABLED
open_on_new = false

[herdr]
open_on_new = false

[paths]
worktrees = "$worktrees"
repos     = "$repos"

[git]
base_branch   = "main"
branch_prefix = { Bug = "fix", default = "feat" }

# One block per repo (twig config edit):
# [repos.my-backend]
# label   = "Back"
# short   = "back"
# default = true
# run     = "dotnet watch run"
EOF
  ok "config created: $CONFIG_FILE"
  info "register your repos with \`twig config edit\` (see docs/CONFIG.md)"
}

# ───────────────────────── main ─────────────────────────

printf "${C_BOLD}twig installer${C_OFF} (%s, %s)\n" "$TARGET" "$MODE" >&2

if [ "$MODE" = install ]; then
  setup_git
  setup_herdr
  setup_vscode
  setup_provider
  setup_ai
else
  have git || warn "git not found"
  have herdr || warn "herdr not found"
fi

download_release
install_binary
install_skill
install_herdr_plugin
install_completions

if [ "$MODE" = install ]; then
  write_config
  section "twig doctor"
  "$TWIG" doctor || true
fi

printf "\n" >&2
ok "done: twig ${VERSION#v}"
