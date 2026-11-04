# `twig watch`

Tag a work item (an `agent` tag on Azure DevOps, an `agent` label on GitHub) and twig starts the
card for you: `twig watch` polls the provider and, for each tagged item, runs
`twig new <id> --template <template>`. With an `agent_prompt` in the template, the agents are
already working when you sit down.

> **Read [Security](#security) before turning it on.** Nobody reviews the card before its agent
> starts. Whoever can tag or edit an item can make code run on your machine.

## Scope of the first version

In: polling, one provider (`provider.default`), rules in `watch.toml`, a local ledger, a foreground
loop, `--once`, `--dry-run`, a user service on Linux and macOS, `doctor` integration.

Out (future issues): webhooks, relays, several providers at once, a template chosen by the tag
itself (`agent:<template>`), a server/VM mode. The source of events and the executor are separate
(`watch/engine.rs`: `Source`, `Executor`), so a webhook can feed the same rules later.

## `watch.toml`

Next to `config.toml` (`~/.config/twig/watch.toml`; `TWIG_WATCH` overrides the path). Optional:
without it `twig watch` says how to create one. Example: [`examples/watch.toml`](../examples/watch.toml).

```toml
interval       = "60s"   # at least 15s; every wait gets ±10% of jitter
assigned_to_me = true    # default true
max_per_cycle  = 2       # cards started per cycle
max_active     = 5       # cards created by the watch alive at once (in the worktrees folder, not parked)
timeout        = "30m"   # per `twig new`
on_trigger     = ""      # optional shell command; gets TWIG_CARD, TWIG_TEMPLATE, TWIG_WATCH_RESULT (ok|fail)
mark_tag       = ""      # optional tag added to the item after a successful start
start_herdr    = false   # let `twig new` start the herdr server (see Running it)

[[rules]]
tag      = "agent"
template = "empreitada"  # required, must exist in templates.toml; there is no fallback template
project  = "Frota"       # az: required
# repo   = "RafaelKC/twig"          # gh: required (owner/repo)
# trusted_taggers = ["RafaelKC"]    # gh: who may add the tag (default: the authenticated user)
```

- **Rules.** One per `(tag, scope)`; the scope is explicit (`project` with `az`, `repo` with `gh`)
  and there are no wildcards. The same tag in another scope can use another template: that is how
  you pick a template per project or repo. An item matches at most one rule per tag; two rules for
  the same tag and scope are a validation error. The tag match is exact and case-insensitive:
  `agent` does not match `agent:done`.
- **`assigned_to_me`.** The query uses `@Me` (az) / `--assignee @me` (gh). With `false`, `doctor`
  warns: without a remote lock two machines can start the same card.
- **Limits.** Items over `max_per_cycle` / `max_active` wait for a later cycle and are **not**
  written to the ledger. They protect you from 40 cards tagged at once, or from a laptop that
  wakes up after a weekend.
- **`mark_tag`.** Off by default. On Azure DevOps it rewrites `System.Tags` (read, add, write),
  which can race with someone editing the tags; the local ledger is the source of truth anyway.
- **Reloading.** The config, the templates and `watch.toml` are read again at the start of every
  cycle. If the file becomes invalid, the error is logged and the last valid one is kept.

`twig watch validate` (also in `twig doctor`) checks: the templates exist, each rule has a scope
that fits the provider, no duplicate rules, the interval, and warns about templates without
`agent_prompt` (the agent would start idle) and about agents run with permissions bypassed.

## How a cycle decides

1. One query per scope: a WIQL per project on Azure DevOps (all the tags of that project in one
   query, so one `az` call), one `gh issue list` per repo and tag on GitHub (core API, not the
   30-requests-per-minute search). Closed items are ignored.
2. An item that is **not in the ledger** and matches a rule is **new**: the watch fires on
   *entering* the set, not on every cycle.
3. If the card folder already exists (a parked card keeps its folder), nothing is created; the
   item is recorded as `exists`, so a later `twig rm` while the tag stays does not bring it back.
4. The entry is written to the ledger as `pending` **before** `twig new` runs and updated to `ok`
   or `fail` after. A crash in the middle does not start the card again.
5. When an item leaves the set (tag removed, unassigned, closed), its entry is removed. To start
   it again: take the tag off and put it back, or `twig watch forget <id>`.
6. **Only scopes whose query succeeded are pruned.** If the query of a project fails (network,
   expired token), its entries are left alone; otherwise a network outage would wipe the ledger
   and re-fire every card when the network returned. A failing scope backs off exponentially, up
   to 10 minutes, and goes back to normal on the first success.
7. `fail` is never retried by itself. It shows in `twig watch status` and in the log; retry with
   `forget` or by tagging again.

Ledger: `<state>/twig/watch.json` (`~/.local/state` on Linux; the data dir on macOS and Windows).
Per card: `<state>/twig/watch/logs/<id>.log` has the output of `twig new`.

## How the card is started

`twig new <id> --template <template>` as a **subprocess** of the same binary, with no tty
(`stdin` is `/dev/null`), `TWIG_NO_UPDATE_CHECK=1`, no `-H`/`-c` (no VS Code window, nothing
attached to herdr), a timeout (`timeout`) and its output in the card's log file. One at a time.
Being a subprocess, a broken card cannot take the loop down. `twig new` rolls back on failure.

Without a tty, a repo that is not in your config is not registered (the prompt gets no answer,
and `twig new` cancels), and a card with several branches on origin takes the first option. The
watch never registers new repos by itself. Both cases show in the card's log.

On shutdown (SIGTERM, Ctrl+C) the `twig new` in progress is waited for, up to its timeout.

## Commands

```
twig watch [--once] [--dry-run]     # loop (or one cycle); --dry-run lists what would start, changes nothing
twig watch status                   # service + ledger (pending/ok/fail/exists, when, log of failures)
twig watch forget <id>              # take it out of the ledger so it can start again
twig watch show | edit | validate
twig watch install | uninstall      # user service (Linux, macOS)
```

`--once` exits non-zero if a query or a start failed. Only one real watch can run at a time (a
lock file in the state folder); `--dry-run` takes no lock.

## Running it

**Simplest, on every platform: `twig watch` in a herdr pane.** It inherits the right environment
(your `PATH`, `SSH_AUTH_SOCK`, the `gh`/`az` logins) and it is the server's own environment that
the agents get.

### Linux (systemd user service)

`twig watch install` writes `~/.config/systemd/user/twig-watch.service` and starts it. Logs:
`journalctl --user -u twig-watch`.

- `KillMode=process` is set on purpose. `twig new` may start the herdr server as a child of the
  service; with the default `control-group` a restart or stop would kill herdr **and every
  agent**.
- A service does not inherit your shell. `install` records your current `PATH` (and `TWIG_CONFIG`,
  `TWIG_TEMPLATES`, `TWIG_WATCH`, `TWIG__*`) in the unit, and runs
  `twig watch --once --dry-run` **with that environment** before enabling it, so a missing tool or
  a `gh` token that lives in a keyring without DBus fails there with a clear message.
- **herdr.** A herdr server started by the service passes the service's minimal environment (no
  `SSH_AUTH_SOCK`, none of your `.bashrc` variables) to *all* its panes. So by default
  (`start_herdr = false`) the watch only uses a herdr server that is already up; if there is none,
  it logs and defers the start. Set `start_herdr = true` if you accept that.
- To keep it running with no session open: `sudo loginctl enable-linger $USER` (not done for you:
  it needs sudo on some distros).
- **WSL2** needs `systemd=true` under `[boot]` in `/etc/wsl.conf` (`install` checks and tells you).
  WSL also shuts the distro down when no terminal is open, and the watch stops with it.

### macOS (LaunchAgent)

`~/Library/LaunchAgents/dev.twig.watch.plist` with `RunAtLoad`, `KeepAlive`, the `PATH`, the log
in `<state>/twig/watch/launchd.log` and `AbandonProcessGroup` (launchd's `KillMode=process`),
loaded with `launchctl bootstrap gui/$UID`.

### Windows

No `install` yet; it prints the command to run once:
`schtasks /Create /SC ONLOGON /TN twig-watch /TR "\"C:\path\twig.exe\" watch"`. Or run
`twig watch` in a herdr pane.

## Security

`twig new` run by hand has a person reading the card first. Here there is no one.

1. **Who tagged (GitHub).** The repo may be public. For each *new* candidate the watch reads the
   issue timeline (`gh api repos/<o>/<r>/issues/<n>/timeline`), takes the last `labeled` event of
   the tag, and requires the actor to be in the rule's `trusted_taggers` (default: you, from
   `gh api user`). Otherwise the item is ignored, logged and never written to the ledger as `ok`.
   Azure DevOps: the organization is private and the project ACL says who can edit; the first
   version relies on `assigned_to_me` plus the project scope.
2. **Prompt injection.** The card's description and acceptance criteria go into the agent's
   context file, and the agent starts with `agent_prompt` and nobody in the loop. Give the
   templates used by the watch an `[agent] command` **without** `--dangerously-skip-permissions`
   or `bypassPermissions`: for Claude Code, `--permission-mode acceptEdits` with a restricted
   `--allowedTools`. `watch validate` and `doctor` warn about the bypass. Consider keeping
   `GH_TOKEN` out of the agent's environment too.
3. **Commands versioned in a repo.** A repo's `.twig.toml` can define `install`, `post_create` and
   `run`, and the watch runs them unattended. It only applies to repos already in your config, so
   the risk is that of doing it by hand, but know it. The watch never registers new repos.
4. **External input never becomes shell or a path.** The id is checked (`^[0-9]+$` on az,
   `owner/repo#N` on gh) before anything else and passed as argv, never through a shell. The
   template comes from your config, never from the item. (`on_trigger` is your own command; the
   values reach it as environment variables.)
5. **Credentials.** None stored; the watch reuses the `az`/`gh` login. The log has no card title
   or body: only the id, rule, template, result and duration.
6. **Cost and abuse.** `max_per_cycle` and `max_active` (above).
7. **Webhooks (future).** Nothing listening publicly by default; `127.0.0.1`, HMAC/secret, the
   same validation.
