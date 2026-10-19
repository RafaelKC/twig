<div align="center">

[🇺🇸 English](README.md) | 🇧🇷 Português

# 🌿 twig

**Um workspace isolado por card.**
Git worktrees em todos os repos que uma tarefa toca, um agente de IA por repo no
[herdr](https://herdr.dev) e um workspace do VS Code colorido, tudo com um comando.

![Rust](https://img.shields.io/badge/rust-2021-orange?logo=rust)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)
![Status](https://img.shields.io/badge/status-early-yellow)

</div>

---

```console
$ twig new 52644
→ card: Vehicle category per route
→ [api] fetch...
✔ [api] feat/52644-vehicle-category-per-route
→ [web] fetch...
✔ [web] feat/52644-vehicle-category-per-route
✔ VS Code workspace generated (color #af601a, compound with 2 processes)
✔ CLAUDE.md generated in ~/.wt/52644
✔ herdr workspace #52644 created
```

## Por quê

Uma tarefa de verdade raramente vive em um repo só. Back end, front end e uma biblioteca
compartilhada mudam juntos, e trocar de branch nos clones principais para equilibrar duas
tarefas quebra tudo o que está rodando. O `twig` dá a cada card (um work item do Azure
DevOps ou uma issue do GitHub) o seu próprio sandbox:

```
~/.wt/52644/
├── api/                    git worktree na branch do card
├── web/                    git worktree na branch do card
├── CLAUDE.md               (AGENTS.md no Codex) título, descrição, critérios de aceite, repos e regras do card
├── 52644.code-workspace    workspace do VS Code, colorido por card, com tasks de run e um compound de debug
└── .ado.json               work item em cache
```

Seus clones principais ficam na `main`. Dois cards nunca pisam um no outro.

## Funcionalidades

- **Worktrees em vários repos.** Um `twig new <id>` cria um worktree por repo. A branch é
  escolhida por você: uma branch existente no `origin` com o id no nome, ou uma nova
  `feat/<id>-<slug>` (ou `fix/` para bugs), com o slug vindo do título do card.
- **Contexto do card para o agente.** Título, estado, descrição e critérios de aceite são
  puxados do Azure DevOps (`az`) ou do GitHub (`gh`) para o `CLAUDE.md` do card (`AGENTS.md` no Codex), junto com uma
  seção de "contexto do card" que sobrevive à regeneração.
- **Workspace no herdr.** Uma aba `agent` com um agente de IA por repo e uma aba `run` que
  inicia cada projeto. Fechou um painel sem querer? `twig add-repo` traz de volta.
- **Workspace no VS Code.** Nomes de pasta amigáveis, cor da barra de título por card, tasks
  `run:` e um compound que depura todos os repos de uma vez.
- **Arquivos locais.** Arquivos no gitignore (`appsettings.Development.json`, `.env.local`, …)
  são copiados do clone principal para cada worktree.
- **Uma só config.** Tudo que é específico do time fica em `~/.config/twig/config.toml`.
- **Autodiagnóstico.** `twig doctor` verifica ferramentas, logins, config e repos, e encontra
  worktrees órfãos.

## Layout do herdr

```
┌─ workspace "52644" ─────────────────────────────────────────┐
│ [agent]  run                                                │
│ ┌─────────────────────────┬─────────────────────────┐       │
│ │ api                     │ web                     │       │
│ │ $ claude                │ $ claude                │       │
│ │                         │                         │       │
│ └─────────────────────────┴─────────────────────────┘       │
└─────────────────────────────────────────────────────────────┘
```

## Requisitos

O instalador cuida disso.

| Ferramenta | |
|---|---|
| `git` | obrigatório |
| [`herdr`](https://herdr.dev) | obrigatório |
| `az` (+ extensão `azure-devops`) **ou** `gh` | pelo menos um, para ler os cards |
| `claude` ([Claude Code](https://claude.com/claude-code)) | o agente de IA iniciado por repo |
| `code` (VS Code) | opcional |

Detalhes em [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md).

## Instalação

Linux (x86_64, arm64, WSL) e macOS (Intel, Apple Silicon):

```bash
curl -fsSL https://rndm.chicovis.dev/twig/unix-install | sh
```

O instalador verifica e instala as dependências (git, herdr, az/gh, Claude Code,
opcionalmente VS Code), configura os logins, baixa a release para o seu sistema, confere o
checksum e a [assinatura Sigstore](docs/RELEASING.md#signing-and-verification) e instala o
twig, a skill de IA, o plugin do herdr e os completions do shell. Rodar de novo atualiza.
Detalhes: [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md).

Depois registre seus repos e confira tudo:

```bash
twig config edit
twig doctor
```

| | |
|---|---|
| Atualizar | `twig update` |
| Versão específica | `curl -fsSL https://rndm.chicovis.dev/twig/unix-install \| sh -s -- --version v0.2.0` |
| Desinstalar | `twig uninstall` |

<details>
<summary>Compilar do código-fonte</summary>

```bash
git clone https://github.com/RafaelKC/twig && cd twig
cargo build --release
ln -s "$PWD/target/release/twig" ~/.local/bin/twig
mkdir -p ~/.config/twig && cp examples/config.toml ~/.config/twig/config.toml
twig completions bash > ~/.local/share/bash-completion/completions/twig
```

</details>

## Uso

```bash
twig new 52644                      # cria o card com os repos padrão
twig new 52644 --repos api,ui-kit   # escolhe os repos
twig add-repo 52644 web             # adiciona um repo depois (ou restaura painéis fechados)
twig open 52644                     # abre o card no destino padrão (herdr ou code)
twig open-card 52644              # abre a página do card (Azure DevOps) no navegador
twig herdr 52644                    # abre o card no herdr
twig code                           # abre o card atual no VS Code
twig ls                             # cards, branches, alterações não commitadas (*)
twig status 52644                   # ahead/behind, arquivos alterados, agente do herdr
twig refresh 52644 --refetch        # regenera CLAUDE.md / workspace, rebusca o card
twig rm 52644 --branches            # terminou: remove worktrees (e branches locais)
```

| Comando | O que faz |
|---|---|
| `new <id> [slug]` | worktrees, `CLAUDE.md`/`AGENTS.md`, workspace do herdr e, se habilitado, o do VS Code |
| `add-repo <id> <repo,...>` | adiciona repos a um card; recria painéis do herdr que faltam |
| `open [id] [default\|herdr\|code]` | abre um card no destino escolhido; padrão vem de `[open] default` (herdr) |
| `open-card [id]` (`opcd`) | abre a página do card no navegador (só Azure DevOps por enquanto) |
| `herdr <id>` | abre/foca o card no herdr |
| `code [id]` | abre o card no VS Code; sem id, o card atual |
| `ls` | cards ativos e a branch de cada repo |
| `status [id]` | por repo: branch, arquivos alterados, ahead/behind vs base e origin; workspace e agente do herdr |
| `refresh <id>` | regenera o `CLAUDE.md`/`AGENTS.md` e o `.code-workspace` |
| `rm <id> [-f]` | remove o card e o workspace do herdr (recusa se houver alterações não commitadas ou agente rodando; `-f` ignora) |
| `prune` | limpa registros de worktrees órfãos |
| `doctor` | verifica dependências, logins, config e repos |
| `config edit` | edita a config e valida ao salvar |
| `update` | atualiza para a última release |
| `uninstall` | remove o twig, seu plugin e a skill |
| `completions <shell>` | script de completion do shell |

Todas as opções: [docs/COMMANDS.md](docs/COMMANDS.md).

## Configuração

```toml
version = 1

[provider]
default = "az"                      # ou "gh"

[paths]
worktrees = "~/.wt"
repos     = "~/dev"

[repos.api]
label   = "⚙️ API"                  # nome da pasta no VS Code
short   = "api"                     # nome do painel no herdr
default = true                      # faz parte de todo card novo
run     = "dotnet watch run --project src/Api"
install = "dotnet restore"
local_files = ["src/*/appsettings.Development.json"]

[repos.web]
label   = "🌐 Web"
default = true
run     = "pnpm start"
```

Exemplo completo: [examples/config.toml](examples/config.toml). Todos os campos:
[docs/CONFIG.md](docs/CONFIG.md).

## Extras

| | |
|---|---|
| [herdr-plugins/twig](herdr-plugins/README.md) | plugin do herdr: `prefix+shift+o` abre o card em foco no VS Code |
| [skills/twig](skills/twig/SKILL.md) | skill de agente de IA: como usar o twig, mais como criar abas, painéis e workspaces customizados no herdr |

## Estrutura do projeto

```
src/              a CLI twig (Rust)
tests/            testes de integração (ferramentas falsas num PATH controlado)
examples/         config de exemplo
herdr-plugins/    plugin do herdr
skills/           skill de agente de IA
scripts/          helper de release
.github/          workflow de release
install.sh        instalador
docs/             comandos, config, dependências, release, roadmap
refs/             referências avaliadas mas não usadas
```

```bash
cargo test        # testes unitários + de integração
```

## Status

Inicial e opinativo: feito para um fluxo diário com Azure DevOps + .NET + Angular em
Linux/WSL, agora generalizado pela config. Veja o [roadmap](docs/ROADMAP.md) para o que vem
a seguir (portas por card, mais ações do herdr, Windows nativo).

## Licença

[MIT](LICENSE)
