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
├── MEMORY.md               memória do card (symlink; sobrevive ao `twig rm`)
├── .twig-ports.env         as portas de dev do card (4244, 5044: as mesmas para este card, sempre)
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
  inicia cada projeto. Fechou um painel sem querer? `twig add-repo` traz de volta. `twig new
  --prompt "/entregar {id}"` (ou `agent_prompt` num template) já inicia os agentes trabalhando.
- **Portas por card.** Um repo com `port = 4200` (e extras nomeadas como `ports = { https = 4300 }`) roda em portas próprias em cada card,
  determinística por card (card 52644 → `4244`), então vários cards rodam ao mesmo tempo. Todo
  processo do card recebe a porta de todos os repos (`TWIG_PORT_<REPO>`, `PORT`), e o `env` do
  repo as traduz para o que o projeto lê: o front chama a API do mesmo card, a API libera esse
  front no CORS.
- **Pull requests e limpeza.** `twig pr` faz push e abre um PR por repo, ligados ao card e entre
  si; `twig clean` remove os cards com branches mergeadas ou work item fechado.
- **GitHub ou Azure DevOps.** `twig new 52644` ou `twig new acme/web#12`; os dois providers
  ficam atrás de uma mesma interface.
- **Workspace no VS Code.** Nomes de pasta amigáveis, cor da barra de título por card, tasks
  `run:` e um compound que depura todos os repos de uma vez.
- **Arquivos locais.** Arquivos no gitignore (`appsettings.Development.json`, `.env.local`, …)
  são copiados do clone principal para cada worktree.
- **Hooks de ciclo de vida.** Seus próprios comandos de shell em `post_create`, `pre_rm`, `post_rm`, `post_open` e `post_refresh`, no card todo ou por repo; `--no-hooks` ignora todos.
- **Templates de card.** Presets nomeados em `~/.config/twig/templates.toml` (repos, agente, install, hooks): `twig new 123 --template fullstack`; as flags têm precedência.
- **Uma config, mais a do repo.** Suas configurações ficam em `~/.config/twig/config.toml`; um
  repo pode versionar as dele (`run`, `install`, `port`, `env`) num `.twig.toml`, um card pode
  sobrescrevê-las em `<card>/.twig.toml`, e variáveis `TWIG__SECAO__CHAVE` sobrescrevem tudo.
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

### Windows

Duas opções:

- **WSL (recomendado, estável):** abra sua distro WSL e rode o comando de Linux acima. O twig,
  o herdr e os agentes rodam dentro do WSL; o VS Code do Windows abre os cards pela extensão
  WSL.
- **Windows nativo (beta):** no PowerShell (5.1 ou 7):

  ```powershell
  irm https://github.com/RafaelKC/twig/releases/latest/download/install.ps1 | iex
  ```

  > **O suporte nativo do herdr a Windows está em beta.** O twig funciona nele, mas painéis,
  > detecção de agentes e o plugin do herdr podem falhar. O instalador avisa antes de mudar
  > qualquer coisa, e o `twig doctor` continua lembrando.

  Ele instala as dependências que faltam com o winget (Git, GitHub CLI ou Azure CLI, VS Code)
  e os instaladores oficiais (herdr, Claude Code), coloca o `twig.exe` em
  `%LOCALAPPDATA%\Programs\twig` (adicionado ao PATH do usuário), a skill, o plugin do herdr
  (`prefix+shift+o`) e os completions no seu perfil do PowerShell. A config fica em
  `%APPDATA%\twig\config.toml`. **Não precisa de admin** para o twig em si (só pastas do
  usuário e o PATH do usuário); as dependências que faltam são instaladas por usuário quando dá
  (Git, VS Code, gh, herdr, Claude Code). Só a Azure CLI é instalada para a máquina toda, então
  instalá-la mostra o pedido de admin do Windows (UAC). Com `irm | iex`, as opções vão por variável de ambiente
  (`$env:TWIG_VERSION = "v0.4.0"`), ou como parâmetros se você salvar o script
  (`.\install.ps1 -Version v0.4.0`). `twig update` e `twig uninstall` funcionam igual ao Linux.

  No Windows, `install`/`post_create` e os hooks rodam via `cmd /C` e o `run` no shell do
  painel do herdr (PowerShell), então prefira o [`env`](docs/CONFIG.md#repo-environment-env)
  do repo (resolvido pelo twig em qualquer SO) a `${PORT:-4200}`, que é sintaxe de `sh`.

Depois registre seus repos e confira tudo:

```bash
twig config edit
twig doctor
```

| | |
|---|---|
| Atualizar | `twig update` (o twig avisa no stderr quando há release nova; desative com `TWIG_NO_UPDATE_CHECK=1` ou `[update] check = false`) |
| Versão específica | `curl -fsSL https://rndm.chicovis.dev/twig/unix-install \| sh -s -- --version v0.2.0`; Windows: `$env:TWIG_VERSION = "v0.4.0"` antes do `irm \| iex` |
| Desinstalar | `twig uninstall` |

<details>
<summary>Compilar do código-fonte</summary>

```bash
git clone https://github.com/RafaelKC/twig && cd twig
cargo build --release
ln -s "$PWD/target/release/twig" ~/.local/bin/twig
mkdir -p ~/.config/twig && cp examples/config.toml ~/.config/twig/config.toml
echo 'source <(COMPLETE=bash twig)  # twig completions' >> ~/.bashrc
```

</details>

## Uso

```bash
twig new 52644                      # cria o card com os repos padrão
twig new 52644 --repos api,ui-kit   # escolhe os repos
twig add-repo 52644 web             # adiciona um repo depois (ou restaura painéis fechados)
twig open 52644                     # abre o card no destino padrão (herdr ou code)
twig open-card 52644              # abre a página do card no navegador
twig new acme/web#12               # GitHub: issue 12 de acme/web (pasta do card web-12)
twig herdr 52644                    # abre o card no herdr
twig code                           # abre o card atual no VS Code
twig ls                             # cards, branches, alterações não commitadas (*)
twig status 52644                   # ahead/behind, arquivos alterados, agente do herdr
twig refresh 52644 --refetch        # regenera CLAUDE.md / workspace, rebusca o card
twig memo "validação foi pra API"   # (em desenvolvimento) nota para a próxima sessão do agente
twig rm 52644 --branches            # terminou: remove worktrees (e branches locais)
```

| Comando | O que faz |
|---|---|
| `new <id> [slug]` | worktrees, `CLAUDE.md`/`AGENTS.md`, workspace do herdr e, se habilitado, o do VS Code |
| `add-repo <id> <repo,...>` | adiciona repos a um card; recria painéis do herdr que faltam |
| `open [id] [default\|herdr\|code]` | abre um card no destino escolhido; padrão vem de `[open] default` (herdr) |
| `open-card [id]` (`opcd`) | abre a página do card (work item ou issue) no navegador |
| `herdr <id>` | abre/foca o card no herdr |
| `code [id]` | abre o card no VS Code; sem id, o card atual |
| `ls` | cards ativos e a branch de cada repo (`--json` para scripts) |
| `status [id]` | por repo: branch, arquivos alterados, ahead/behind vs base e origin; workspace e agente do herdr (`--json` para scripts) |
| `refresh <id>` | regenera o `CLAUDE.md`/`AGENTS.md` e o `.code-workspace` |
| `memo [texto] [-c id]` | (em desenvolvimento) acrescenta uma linha datada à memória do card, mantida entre sessões e após o `rm`; sem texto, mostra a memória |
| `sync <id> [--merge\|--rebase] [-n]` | busca a branch base e faz rebase/merge dela em cada repo do card |
| `clean [-n] [-y]` | remove cards terminados (branches mergeadas ou work item fechado), perguntando card a card |
| `ports [id] [--env]` | mostra as portas de dev do card (determinísticas por card, passadas aos panes como `TWIG_PORT_<REPO>` / `PORT`) |
| `pr [id] [--draft] [-n]` | faz push da branch de cada repo e abre um PR por repo, ligados ao card e entre si |
| `rm <id> [-f]` | remove o card e o workspace do herdr (recusa se houver alterações não commitadas ou agente rodando; `-f` ignora) |
| `prune` | limpa registros de worktrees órfãos |
| `doctor` | verifica dependências, logins, config e repos (`--json` para scripts) |
| `template ls` / `show NOME` / `edit` | lista, mostra e edita os templates de card (`new --template NOME` aplica um) |
| `config edit` | edita a config e valida ao salvar |
| `config get <chave>` / `config set <chave> <valor>` | lê / grava um valor por chave com pontos (`git.base_branch`), validado, preservando comentários |
| `update` | atualiza para a última release |
| `uninstall` | remove o twig, seu plugin e a skill |
| `completions <shell>` | hook de completion do shell (o instalador coloca no `~/.bashrc` / `~/.zshrc`); completa também ids de card, repos e templates |

Todas as opções: [docs/COMMANDS.md](docs/COMMANDS.md).

### Memória do card (em desenvolvimento)

Agentes perdem contexto entre sessões. `twig memo "..."` acrescenta uma linha datada a um
`MEMORY.md` por card. O arquivo real fica em `<worktrees>/memos/<id>-<provider>/` e a pasta do
card tem um symlink relativo para ele, então as notas **sobrevivem ao `twig rm`** e voltam
quando `twig new <id>` reabre o card. As últimas 10 entradas são copiadas para o
`CLAUDE.md`/`AGENTS.md` em `new`, `add-repo` e `refresh`. É a primeira versão, simples, da
[#16](https://github.com/RafaelKC/twig/issues/16) e o formato ainda pode mudar; o desenho e o
que falta (consolidação, limite de tamanho, exportar no `rm`) estão em
[docs/MEMORY.md](docs/MEMORY.md) (em inglês).

Do mesmo modo (experimental), o `twig rm` arquiva as sessões do Claude Code dos repos do card em
`memos/<id>-<provider>/sessions/<repo>/` e o `twig new` / `twig add-repo` as restauram; desative
com `[agent] keep_sessions = false`. Transcrições podem conter segredos; veja
[docs/MEMORY.md](docs/MEMORY.md#agent-sessions).

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
port    = 5000                      # 5000 + o offset do card, em cada card
env     = { ASPNETCORE_URLS = "http://localhost:$PORT", Cors__Origins__0 = "http://localhost:${TWIG_PORT_WEB:-4200}" }
local_files = ["src/*/appsettings.Development.json"]

[repos.web]
label   = "🌐 Web"
default = true
run     = "ng serve --port ${PORT:-4200} --proxy-config proxy.conf.js"   # o proxy lê $TWIG_PORT_API
port    = 4200
```

As mesmas configurações de repo podem morar no próprio repo, versionadas, num `.twig.toml`
([exemplo](examples/repo.twig.toml)).

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
docs/             comandos, config, dependências, release, roadmap, agente autônomo
refs/             referências avaliadas mas não usadas
```

```bash
cargo test        # testes unitários + de integração
```

## Status

Inicial e opinativo: feito para um fluxo diário com Azure DevOps + .NET + Angular em
Linux/WSL, agora generalizado pela config. Veja o [roadmap](docs/ROADMAP.md) para o que vem
a seguir (mais ações do herdr, Windows nativo fora do beta junto com o herdr).

## Licença

[MIT](LICENSE)
