//! The card folder's CLAUDE.md.

use std::fs;

use anyhow::Result;

use crate::config::{Config, ProviderKind};
use crate::provider::{self, CardInfo};
use crate::{card, sys, ui, workspace};

const CONTEXT_HEADING: &str = "## Contexto do card";
const CONTEXT_PLACEHOLDER: &str = "_(a preencher)_";

pub fn write(cfg: &Config, id: &str, slug: &str, info: Option<&CardInfo>) -> Result<()> {
    let dir = cfg.card_dir(id);
    let path = dir.join("CLAUDE.md");
    let repos = card::repos_ordered(cfg, &dir);

    // keep the "Contexto do card" section if it has already been filled in
    let kept = fs::read_to_string(&path).ok().and_then(|old| {
        let ctx = &old[old.find(CONTEXT_HEADING)?..];
        (!ctx.contains(CONTEXT_PLACEHOLDER)).then(|| ctx.trim_end().to_string())
    });

    let mut rows = String::new();
    let mut runs = String::new();
    for repo in &repos {
        let branch = sys::git_current_branch(&dir.join(repo)).unwrap_or_else(|| "?".into());
        let main = cfg.repo_path(repo).map(|p| p.display().to_string()).unwrap_or_else(|| "?".into());
        rows += &format!("| {} | `{repo}` | `{branch}` | `{main}` |\n", cfg.label(repo));
        if let Some(r) = cfg.repo(repo).filter(|r| !r.run.is_empty()) {
            runs += &format!("- `{repo}`: `{}`\n", r.run);
        }
    }
    if runs.is_empty() {
        runs = "_(nenhum comando de run configurado para estes repos)_\n".into();
    }

    let mut heading = format!("# Card #{id}");
    match info {
        Some(i) => heading += &format!(" — {}", i.title),
        None if !slug.is_empty() => heading += &format!(" — {slug}"),
        None => {}
    }

    let mut meta = String::new();
    let url = info.map(|i| i.url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| provider::fallback_url(cfg, id));
    if !url.is_empty() {
        meta += &format!("**Work item:** {url}\n");
    }
    let mut pede = String::new();
    if let Some(i) = info {
        if !i.kind.is_empty() {
            let state = if i.state.is_empty() { "?" } else { &i.state };
            meta += &format!("**Tipo:** {} · **Estado:** {state}\n", i.kind);
        }
        if !i.assigned.is_empty() {
            meta += &format!("**Atribuído a:** {}\n", i.assigned);
        }
        if !i.tags.is_empty() {
            meta += &format!("**Tags:** {}\n", i.tags);
        }
        if !i.description.is_empty() || !i.acceptance.is_empty() {
            pede += "\n## O que o card pede\n\n<!-- Copiado do provider pelo twig. Não edite: `twig refresh` sobrescreve. -->\n";
            if !i.description.is_empty() {
                pede += &format!("\n{}\n", i.description);
            }
            if !i.acceptance.is_empty() {
                pede += &format!("\n### Critérios de aceite\n\n{}\n", i.acceptance);
            }
        }
    }

    let debug_note = if workspace::debug_count(cfg, &repos) >= 2 && cfg.vscode.enabled {
        format!(
            "Para debugar tudo de uma vez, o workspace define o compound\n\
             `▶ card #{id}: subir e debugar tudo`, no topo do dropdown de Run and Debug\n\
             (`F5`). Ele compõe as configs que já existem no `launch.json` de cada repo.\n\n"
        )
    } else {
        String::new()
    };

    let vscode_note = if cfg.vscode.enabled {
        format!(
            "As mesmas coisas existem como tasks do VS Code no arquivo `{id}.code-workspace`\n\
             (`run: <repo>` e `twig: subir tudo`).\n\n"
        )
    } else {
        String::new()
    };

    let provider_rule = match cfg.provider.default {
        ProviderKind::Az => "- **O `az` cli está disponível.** Pode consultar o Azure DevOps direto\n  \
             (`az boards work-item show --id <id>`, `az repos pr list`, etc.) em vez de\n  \
             pedir pro usuário.\n",
        ProviderKind::Gh => "- **O `gh` cli está disponível.** Pode consultar o GitHub direto\n  \
             (`gh issue view <id>`, `gh pr list`, etc.) em vez de pedir pro usuário.\n",
    };

    let context = kept.unwrap_or_else(|| {
        format!(
            "{CONTEXT_HEADING}\n\n<!-- Preencha com o que importa: decisões tomadas, arquivos centrais,\n     \
             pontas soltas. Isto sobrevive entre sessões e ao `twig refresh`. -->\n\n{CONTEXT_PLACEHOLDER}"
        )
    });

    let base = &cfg.git.base_branch;
    let dir_s = dir.display();
    let body = format!(
        "{heading}\n\n\
<!-- Gerado pelo twig {version}. `twig refresh {id}` regenera. -->\n\n\
{meta}{pede}\n\
## Onde você está\n\n\
Esta pasta **não é um clone normal**. Cada subdiretório abaixo é uma `git worktree`\n\
dedicada a este card. Os clones principais moram em outro lugar e continuam na\n\
branch base; mexer neles quebra o isolamento do card.\n\n\
| nome | repo (worktree aqui) | branch | clone principal |\n\
|---|---|---|---|\n\
{rows}\n\
Branch base: `{base}`\n\n\
## Como rodar\n\n\
{runs}\n\
{vscode_note}{debug_note}\
## Regras deste diretório\n\n\
- **Trabalhe apenas dentro de `{dir_s}`.** Não edite, não faça checkout e não\n  \
comite nos clones principais listados na tabela: eles são compartilhados por\n  \
todos os cards.\n\
- **Não troque de branch dentro das worktrees.** Se precisar de outra branch, é\n  \
outro card (`twig new`).\n\
- **Cada repo comita separado.** Não existe commit atômico entre eles; quando a\n  \
mudança é acoplada, cite o card (`#{id}`) nas mensagens para dar rastreabilidade.\n\
- **Dependências e caches são por worktree.** Não compartilhe nem crie symlink\n  \
de `node_modules`, `bin/`, `obj/` etc. entre cards.\n\
- **Não comite este arquivo.** Ele mora fora dos repos de propósito.\n\
{provider_rule}\n\
{context}\n",
        version = env!("CARGO_PKG_VERSION"),
    );

    fs::write(&path, body)?;
    ui::ok(format!("CLAUDE.md gerado em {}", dir.display()));
    Ok(())
}
