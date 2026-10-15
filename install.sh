#!/bin/sh
# twig installer.
#
#   curl -fsSL https://raw.githubusercontent.com/RafaelKC/twig/main/install.sh | sh
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
    *) die "opção desconhecida: $1" ;;
  esac
done

# ───────────────────────── system detection ─────────────────────────

case "$(uname -s)" in
  Linux) OS=linux ;;
  Darwin) OS=macos ;;
  *) die "sistema não suportado: $(uname -s) (só Linux e macOS)" ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) die "arquitetura não suportada: $(uname -m)" ;;
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

have curl || die "curl é necessário para instalar o twig"
have tar || die "tar é necessário para instalar o twig"

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
    die "instale as Command Line Tools do Xcode (janela aberta) e rode o instalador de novo"
  fi
  pm_install git git git git git || die "não sei instalar o git aqui; instale manualmente"
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
    *) die "não sei instalar o gh aqui; veja https://cli.github.com" ;;
  esac
}

install_az() {
  case "$PM" in
    brew) brew install azure-cli ;;
    apt-get) curl -fsSL https://aka.ms/InstallAzureCLIDeb | $SUDO bash ;;
    *) die "não sei instalar o az aqui; veja https://learn.microsoft.com/cli/azure/install-azure-cli" ;;
  esac
}

install_vscode() {
  if [ "$IS_WSL" = 1 ]; then
    warn "no WSL o VS Code é instalado no Windows (https://code.visualstudio.com) com a extensão WSL; o \`code\` aparece sozinho"
    return 1
  fi
  if [ "$PM" = brew ]; then
    brew install --cask visual-studio-code
  elif have snap; then
    $SUDO snap install code --classic
  else
    warn "instale o VS Code manualmente: https://code.visualstudio.com/download"
    return 1
  fi
}

install_claude() {
  curl -fsSL https://claude.ai/install.sh | bash
  have claude || PATH="$HOME/.local/bin:$PATH"
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
  elif confirm "git não encontrado (obrigatório). Instalar?" y; then
    install_git
    ok "git instalado"
  else
    die "o git é obrigatório"
  fi

  name=$(git config --global user.name || true)
  email=$(git config --global user.email || true)
  if [ -n "$name" ] && [ -n "$email" ]; then
    ok "identidade do git: $name <$email>"
  elif confirm "user.name/user.email do git não configurados. Configurar agora?" y; then
    [ -n "$name" ] || name=$(ask "nome:" "")
    [ -n "$email" ] || email=$(ask "e-mail:" "")
    [ -n "$name" ] && git config --global user.name "$name"
    [ -n "$email" ] && git config --global user.email "$email"
    if [ -n "$name" ] && [ -n "$email" ]; then
      ok "identidade do git configurada"
    else
      warn "identidade do git incompleta (git config --global user.name/user.email)"
    fi
  else
    warn "identidade do git não configurada (o \`twig doctor\` vai continuar avisando)"
  fi
}

setup_herdr() {
  section "herdr"
  if have herdr; then
    ok "$(herdr --version 2>/dev/null || echo herdr)"
  elif confirm "herdr não encontrado (obrigatório). Instalar?" y; then
    install_herdr
    have herdr || die "herdr instalado, mas não está no PATH; abra um terminal novo e rode o instalador de novo"
    ok "herdr instalado"
  else
    die "o herdr é obrigatório"
  fi
}

setup_vscode() {
  section "VS Code (opcional)"
  if have code; then
    ok "code encontrado"
    VSCODE_ENABLED=true
  elif confirm "VS Code não encontrado. Instalar?" n && install_vscode && have code; then
    ok "VS Code instalado"
    VSCODE_ENABLED=true
  else
    info "seguindo sem VS Code (vscode.enabled = false)"
  fi
}

check_az_login() {
  if ! az extension show --name azure-devops >/dev/null 2>&1; then
    info "adicionando a extensão azure-devops do az"
    az extension add --name azure-devops --only-show-errors || warn "não consegui adicionar a extensão azure-devops"
  fi
  AZ_ORG=$(az devops configure --list 2>/dev/null | sed -n 's/^organization *= *//p')
  if [ -z "$AZ_ORG" ]; then
    AZ_ORG=$(ask "organização do Azure DevOps (ex.: https://dev.azure.com/minha-org, vazio pula):" "")
    [ -n "$AZ_ORG" ] && az devops configure --defaults organization="$AZ_ORG"
  fi
  if az devops project list --top 1 --output none >/dev/null 2>&1; then
    ok "az com acesso ao Azure DevOps"
  elif confirm "az sem acesso ao Azure DevOps. Fazer login agora?" y; then
    run_tty az login || warn "login do az falhou"
  else
    warn "az sem login (az login ou az devops login)"
  fi
}

check_gh_login() {
  if gh auth status >/dev/null 2>&1; then
    ok "gh logado"
  elif confirm "gh não está logado. Fazer login agora?" y; then
    run_tty gh auth login || warn "login do gh falhou"
  else
    warn "gh sem login (gh auth login)"
  fi
}

setup_provider() {
  section "provider de cards (az e/ou gh)"
  chosen=""
  for p in az gh; do
    if have "$p" && confirm "$p já está instalado. Usar?" y; then chosen="$chosen $p"; fi
  done
  if [ -z "$chosen" ]; then
    case "$(ask "escolha um provider: [1] az (Azure DevOps) [2] gh (GitHub)" 1)" in
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
  section "IA"
  # only Claude Code for now; the question is here so other agents can be added
  if have claude; then
    if confirm "Claude Code já está instalado. Usar?" y; then AI=claude && ok "usando Claude Code"; fi
  elif confirm "Claude Code não encontrado. Instalar?" y; then
    install_claude
    have claude && AI=claude && ok "Claude Code instalado"
  fi
  [ -n "$AI" ] || warn "sem IA: a tab agent do herdr não vai ter o que rodar"
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
    [ -n "$VERSION" ] || die "não achei a última release de $REPO"
  fi
  asset="twig-$TARGET.tar.gz"
  base="https://github.com/$REPO/releases/download/$VERSION"
  TMP=$(mktemp -d)
  trap 'rm -rf "$TMP"' EXIT

  info "baixando twig $VERSION ($TARGET)"
  for f in "$asset" SHA256SUMS SHA256SUMS.sigstore.json; do
    if [ -n "${TWIG_DIST_DIR:-}" ]; then
      [ -f "$TWIG_DIST_DIR/$f" ] && cp "$TWIG_DIST_DIR/$f" "$TMP/$f"
    else
      curl -fsSL -o "$TMP/$f" "$base/$f" || [ "$f" = SHA256SUMS.sigstore.json ] || die "falha ao baixar $base/$f"
    fi
  done
  [ -f "$TMP/$asset" ] || die "release $VERSION não tem $asset"

  expected=$(awk -v a="$asset" '$2 == a || $2 == "*"a {print $1}' "$TMP/SHA256SUMS")
  [ -n "$expected" ] || die "$asset não está no SHA256SUMS"
  sha256_check "$TMP/$asset" "$expected" || die "checksum não confere para $asset"
  ok "checksum conferido"

  if [ -f "$TMP/SHA256SUMS.sigstore.json" ] && have cosign; then
    cosign verify-blob \
      --bundle "$TMP/SHA256SUMS.sigstore.json" \
      --certificate-identity-regexp "^https://github.com/$REPO/\.github/workflows/release\.yml@refs/tags/" \
      --certificate-oidc-issuer https://token.actions.githubusercontent.com \
      "$TMP/SHA256SUMS" >/dev/null 2>&1 || die "assinatura Sigstore inválida para $VERSION"
    ok "assinatura Sigstore conferida"
  else
    info "cosign não instalado: assinatura não conferida (o checksum foi). Verificação manual em docs/RELEASING.md"
  fi

  tar -xzf "$TMP/$asset" -C "$TMP"
  PKG="$TMP/twig-$TARGET"
  [ -x "$PKG/twig" ] || die "pacote sem o binário twig"
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
    ok "twig $OLD_VERSION → ${VERSION#v} em $TWIG"
  else
    ok "twig ${VERSION#v} instalado em $TWIG"
  fi

  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
      rc=""
      case "$(basename "${SHELL:-sh}")" in
        bash) rc="$HOME/.bashrc" ;;
        zsh) rc="$HOME/.zshrc" ;;
      esac
      if [ -n "$rc" ] && ! grep -qs "$BIN_DIR" "$rc" && confirm "$BIN_DIR não está no PATH. Adicionar em $rc?" y; then
        # shellcheck disable=SC2016 # $PATH must expand when the rc file runs, not now
        printf '\nexport PATH="%s:$PATH"  # twig\n' "$BIN_DIR" >>"$rc"
        ok "PATH ajustado em $rc (abra um terminal novo)"
      else
        warn "adicione $BIN_DIR ao PATH"
      fi
      ;;
  esac
}

install_skill() {
  dest="$HOME/.claude/skills/twig"
  if [ "$AI" != claude ] && [ ! -e "$dest" ]; then return; fi
  mkdir -p "$(dirname "$dest")"
  rm -rf "$dest"
  cp -R "$PKG/skills/twig" "$dest"
  ok "skill do Claude Code em $dest"
}

install_herdr_plugin() {
  have herdr || { warn "herdr não encontrado: plugin não instalado"; return; }
  pdir="$DATA_DIR/herdr-plugins/twig"
  mkdir -p "$DATA_DIR/herdr-plugins"
  rm -rf "$pdir"
  cp -R "$PKG/herdr-plugins/twig" "$pdir"
  herdr plugin unlink twig >/dev/null 2>&1 || true
  if herdr plugin link "$pdir" >/dev/null 2>&1; then
    ok "plugin do herdr ligado ($pdir)"
  else
    warn "não consegui ligar o plugin; rode: herdr plugin link $pdir"
    return
  fi

  if grep -qs '# twig:begin' "$HERDR_CONFIG"; then
    : # shortcut already added by a previous install
  elif grep -qs 'key = "prefix+shift+o"' "$HERDR_CONFIG"; then
    warn "prefix+shift+o já tem outro atalho no herdr; aponte para \"twig.open-code\" em $HERDR_CONFIG se quiser"
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
    ok "atalho prefix+shift+o → twig.open-code"
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
  "$TWIG" completions "$shell" >"$file" && ok "autocomplete ($shell) em $file"
  if [ "$shell" = zsh ]; then
    info "no zsh, garanta no ~/.zshrc: fpath=(${file%/*} \$fpath)"
  fi
}

write_config() {
  [ -f "$CONFIG_FILE" ] && { ok "config mantido: $CONFIG_FILE"; return; }
  section "config"
  # shellcheck disable=SC2088 # literal ~ on purpose: twig expands it when reading the config
  repos=$(ask "pasta onde ficam os clones dos seus repos [~]:" "~")
  # shellcheck disable=SC2088
  worktrees=$(ask "pasta das worktrees dos cards [~/.wt]:" "~/.wt")
  mkdir -p "$(dirname "$CONFIG_FILE")"
  cat >"$CONFIG_FILE" <<EOF
# twig config. Format: https://github.com/$REPO/blob/main/docs/CONFIG.md
version = 1

[ai]
provider = "claude"
command  = "claude"

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
  ok "config criado: $CONFIG_FILE"
  info "cadastre seus repos com \`twig config edit\` (veja docs/CONFIG.md)"
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
  have git || warn "git não encontrado"
  have herdr || warn "herdr não encontrado"
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
ok "pronto: twig ${VERSION#v}"
