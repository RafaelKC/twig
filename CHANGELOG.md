# Changelog

All notable changes to twig are listed here, newest first. Each version links to its
[GitHub release](https://github.com/RafaelKC/twig/releases), and entries name the pull request
(and issue) they came from.

## [Unreleased]

### Improvements

- `twig park` now saves the card's herdr workspace (every tab, the splits and their proportions, and each pane, including the ones made by hand) in `<card>/.twig-park-layout.json`, with the session id of each agent. `twig open` / `twig herdr` rebuild it: same tabs and layout, every agent back in its own conversation (`claude --resume <id>`), `run` panes relaunched (respecting `autorun`), shells in their folder, all with the card's env. Panes closed before the park stay closed. Opt out with `twig park --no-snapshot`, `twig open --default-layout` or `[park] snapshot = false` / `resume_sessions = false`. (#100)

### Changes

- **Default change:** `[agent] keep_sessions` is now `false`, so `twig rm` no longer archives the agent's conversations to `memos/` unless asked. Use `twig rm --keep-sessions` for one removal (`--no-keep-sessions` overrides a `true` in the config) or set `keep_sessions = true` to get the old behavior. `twig new` / `twig add-repo` still restore whatever was archived. `twig doctor` warns while the config doesn't set the key. (#100)

## [0.9.1] - 2026-10-29

twig v0.9.1 is a small patch release that fixes the Windows installer failing in some PowerShell setups.

### Fixes

- Fixed `install.ps1` aborting at the checksum step with `Get-FileHash` not recognized when Windows PowerShell 5.1 inherited its module path from PowerShell 7 (for example, when started by `npm run` from a pwsh 7 terminal). The installer now resets `PSModulePath` to the machine-level value on Windows PowerShell 5.1, so twig installs normally in these setups. (#95, #94)

## [0.9.0] - 2026-10-27

v0.9.0 fixes `twig open-card` on Azure DevOps so it opens the work item page instead of raw JSON. It also adds a shorter Windows install command.

### Improvements

- The Windows installer now has a short URL: `irm https://rndm.chicovis.dev/twig/windows-install | iex`. It redirects to the latest `install.ps1`, and the README files and release docs use it. The old GitHub Releases URL still works. (#91, #53)

### Fixes

- `twig open-card` (`opcd`) on Azure DevOps now opens the work item page (`https://dev.azure.com/<org>/<project>/_workitems/edit/<id>`) instead of the REST API URL that returned JSON. Cards already cached with the wrong URL are corrected automatically, so no cache cleanup is needed. (#92, #90)

### Internal

- The autonomous agent waits until no check is pending before failing the PR gates, explains empty failure logs and requires `shellcheck`. (#89, #78)
- Release notes are written by Claude in a read-only `notes` job, falling back to GitHub's generated notes. (#89, #75)

## [0.8.0] - 2026-10-26

### Added

- `twig park [id] [-f]` closes the card's herdr workspace and keeps the card: worktrees, branches, memo and agent sessions stay. It refuses while an agent is running (`--force` overrides) and when run from the card's own workspace. `twig open <id>` brings the card back. VS Code can't be closed from the CLI, so `park` asks you to close that window by hand. (#81, #80)
- `twig park` writes `<card>/.parked.json` with `since`, the work item `state` and its `children` (Azure DevOps child links, GitHub sub-issues), refetched from the provider or taken from the cache. `twig ls` and `twig status` show "parked since …" and `parked_since` in `--json`. `twig park --undo` / `twig unpark` remove the marker. The format is in `docs/PARKED.md`. (#86, #83)
- `[agent] resume_sessions` (off by default): an agent pane opened without a prompt resumes the last conversation in that repo (`claude --continue`, `codex resume --last`), for example after `park`, a closed pane or `rm` + `new`. Claude only resumes when a transcript exists. (#82, #44)
- `--branch-for <repo>=<branch>` on `twig new` and `twig add-repo`, repeatable, picks the branch of one repo. Order per repo: `--branch-for`, `--branch`, a branch on origin with the id, then a new branch. Completion suggests `repo=`. (#85, #84)
- `twig run [id] start|stop|restart [--repos a,b]` controls the apps in the card's `run` tab. `stop` sends Ctrl+C to each repo's pane; `start` recalculates the ports, creates missing panes and runs each repo's `run` command (stopping a process already there first); `restart` does both. (#87, #49)

### Fixed

- `twig run` re-exports the card env when it reuses a pane and reports a failed Ctrl+C. (#87)

## [0.7.0] - 2026-10-26

### Added

- herdr popup launcher on `prefix+shift+s`: type a twig command without the `twig` prefix (or any shell command), see its full output and close the popup with enter. The card env is stripped, so commands run as from outside the card. The installer registers the shortcut, also on existing installs, and `twig uninstall` removes every twig block. (#77, #76)

## [0.6.0] - 2026-10-26

### Added

- Restore the twig environment in herdr panes after a reboot (#73, #72):
  - every pane twig creates gets the markers `TWIG_CARD_ID` and `TWIG_PANE_REPO`;
  - `twig env [id] [--repo X]` prints the `export` lines of a card pane (markers, `TWIG_PORT_*`, `PORT`, the repo's `env`), recomputed from the current config; `eval "$(twig env)"` rebuilds a pane by hand;
  - `install.sh` adds a bash/zsh rc hook that runs `twig env --auto` in herdr panes that lost the env (never outside herdr); `twig uninstall` removes it;
  - `twig doctor` warns when a herdr pane inside a card has no twig env.

### Fixed

- The pane repo marker is `TWIG_PANE_REPO`, so it no longer clashes with the `TWIG_REPO` override used by self-update. (#73)

## [0.5.0] - 2026-10-26

### Added

- `autorun` in card templates, as a bool for every repo or a per-repo table; it overrides the repos' own `autorun` for the whole card. `twig template show` prints it and `twig doctor` validates it. (#62, #54)
- Short aliases for common flags (`new -t NAME -r a,b`, `-j` for `--json`, `-d` for `--draft`, …) and commands (`ar` for `add-repo`, `st` for `status`), listed in "Short aliases" in `docs/COMMANDS.md`. Aliases show in help and completions. (#68, #46)
- `twig rm` archives each repo's Claude Code sessions next to the card memo, and `twig new` / `twig add-repo` restore them. `[agent] keep_sessions` (default `true`) turns it off. Experimental. (#69, #55)

### Fixed

- `twig code` and herdr's `prefix+shift+o` find VS Code's `code` binary outside `PATH` (common install folders on Linux, macOS and Windows). (#66, #50)

### Performance

- `twig ls` probes repos in parallel, with bounded concurrency and a single `git status` call per repo. (#67, #51)

### Internal

- Issue forms for bugs and enhancements, auto-assignment of new issues. (#52)
- Autonomous issue-to-PR agent: the `agent` label makes a self-hosted runner implement the issue with Claude Code and open a reviewed PR (`docs/AGENT.md`). Follow-ups: the PR is assigned to the maintainer, label events no longer displace a queued run, failures are always reported, and issues opened by others are processed when the maintainer labels them. (#57, #59, #60, #61, #65)
- Releases can be cut from GitHub Actions (`release-prepare` → auto-merged PR → `release-tag`). (#70)

## [0.4.1] - 2026-10-22

### Fixed

- Azure DevOps: `--organization` is passed after the `az` subcommand. With `provider.az.organization` set, card data failed silently, so branches lost the slug and the `fix/` prefix, the context file had no title or description, and `twig pr` couldn't work on Azure DevOps. (#47)

## [0.4.0] - 2026-10-21

### Added

- GitHub repo-scoped card ids: `owner/repo#123`, `repo#123` or the issue URL. The card folder and herdr workspace are `<repo>-<number>`, so the same number in two repos no longer collides. `twig open-card` works with GitHub. (#31, #4)
- `twig pr [id] [--draft] [-n]` pushes each repo's branch and opens its PR (`gh pr create` / `az repos pr create`), linked to the card, with a list of the card's other PRs. Existing PRs are printed, not touched. (#32, #12)
- `twig clean [-n] [-y] [--branches] [-f]` removes finished cards: work item closed, or every repo's branch merged (including squash merges). It asks per card unless `--yes`. (#33, #10)
- Per-card dev server ports: `[repos.<name>] port = 4200` gets a deterministic offset per card (`TWIG_PORT_<REPO>`, `PORT`), set on herdr panes, hooks and VS Code, written to `.twig-ports.env` and shown by `twig ports [id] [--env]` and `twig status`. Shared ports between cards are warned about. (#35, #3)
- Named ports per repo (`ports = { https = 5100 }` → `TWIG_PORT_<REPO>_<NAME>`), and a check that base ports are at least `ports.span` apart. (#39)
- Config overrides, strongest first: CLI flag > `TWIG__SECTION__KEY` env vars > `<card>/.twig.toml` > the repo's versioned `.twig.toml` > user config. `twig doctor` lists the overrides in effect. (#36, #19)
- `twig new --prompt TEXT` and the template field `agent_prompt` start the card's agents with a first prompt (`{id}` and `{repo}` are expanded). (#37, #5)
- Dynamic shell completions that complete card ids, repos and templates and never go stale. The installer enables them in `~/.bashrc`, `~/.zshrc` and fish. (#41)
- Native Windows support (beta): `install.ps1`, a `x86_64-pc-windows-msvc` release, `twig update` / `uninstall` on Windows and a Windows CI job. WSL stays the recommended path. (#42)

### Changed

- Azure DevOps and GitHub sit behind one `Provider` interface; no behavior change. (#34)
- Docs pass: READMEs, skill, herdr reference and a new `examples/repo.twig.toml`. (#38)

### Fixed

- `twig update` also refreshes the skill installed for Codex (`~/.agents/skills`, `~/.codex/skills`). (#40)

## [0.3.0] - 2026-10-19

### Added

- `twig config get/set` with dotted keys, validated before writing, comments preserved. (#27, #19)
- Lifecycle hooks `post_create`, `pre_rm`, `post_rm`, `post_open` and `post_refresh` at card and repo level, with `TWIG_*` env vars and `--no-hooks`. A failing `pre_*` hook aborts. (#27, #15)
- Card templates in `~/.config/twig/templates.toml`: `twig new <id> --template NAME`, `twig template ls|show|edit`, `[templates] default`. (#27, #14)
- `twig memo "text"` (in development): a per-card `MEMORY.md` that survives `twig rm`; its last entries go into the card's context file. (#26, #16)

## [0.2.5] - 2026-10-19

### Added

- `twig status [id]`: branch, uncommitted changes, ahead/behind, herdr workspace and running agents per repo. (#24, #9)
- `twig sync <id>` fetches and merges or rebases the base branch into every repo of the card (`--merge`, `--rebase`, `--dry-run`, `[git] sync`). (#24, #13)
- `--json` on `ls`, `status` and `doctor`. (#24, #11)
- Cached, non-blocking hint on stderr when a new release exists (`[update] check`, `TWIG_NO_UPDATE_CHECK`). (#24, #20)

### Fixed

- The card is inferred from a canonicalized working directory (macOS `/private/var`). (#24)

### Internal

- CI on pull requests (fmt, clippy, tests, shellcheck, macOS) and integration tests for `new`, `add-repo`, `rm` and `prune`. (#23, #24, #18)
- `scripts/release.sh` releases through a `release/vX.Y.Z` PR and tags after the merge (`--tag`). (#24, #22)

## [0.2.0] - 2026-10-19

### Added

- `twig open [id] [default|herdr|code]` with `[open] default`; `herdr` and `code` stay as aliases. (#21, #1)
- `twig open-card` / `opcd` opens the card page in the browser (Azure DevOps). (#21, #7)
- Codex alongside Claude Code: `[agent] provider`, `AGENTS.md`, installer prompt, doctor check and skill in `~/.agents/skills`. (#21, #2)
- `worktree.auto_install` and per-repo `post_create` commands.
- pt-BR README and the `rndm.chicovis.dev` install URL.

### Changed

- `twig rm` closes the card's herdr workspace and refuses while an agent is running; `-f/--force` bypasses the checks. (#21, #6)

## [0.1.1] - 2026-10-15

### Changed

- Runtime messages, installer and the generated `CLAUDE.md` are in English.
- The installer installs from the latest release instead of `main`.

## [0.1.0] - 2026-10-15

First release: twig, a Rust CLI that replaces `card.sh`.

### Added

- Per-card environment: git worktrees across repos, a herdr workspace with an agent per repo, a VS Code workspace and a `CLAUDE.md` with the card's context.
- Commands `new`, `add-repo`, `herdr`, `code`, `ls`, `refresh`, `rm`, `prune`, `doctor`, `config edit`, `completions`, `update` and `uninstall`.
- Config in `~/.config/twig/config.toml`.
- Providers: Azure DevOps (`az`) and GitHub (`gh`).
- herdr plugin and AI skill.
- `install.sh` and a release workflow with checksums and Sigstore signatures.

[0.9.1]: https://github.com/RafaelKC/twig/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/RafaelKC/twig/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/RafaelKC/twig/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/RafaelKC/twig/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/RafaelKC/twig/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/RafaelKC/twig/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/RafaelKC/twig/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/RafaelKC/twig/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/RafaelKC/twig/compare/v0.2.5...v0.3.0
[0.2.5]: https://github.com/RafaelKC/twig/compare/v0.2.0...v0.2.5
[0.2.0]: https://github.com/RafaelKC/twig/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/RafaelKC/twig/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/RafaelKC/twig/releases/tag/v0.1.0
