# Autonomous agent (issue → PR)

Add the `agent` label to an open issue and a self-hosted runner (an ARM Ubuntu box) runs Claude Code
to resolve it:

```
issue + your comments ─▶ implement ─▶ push + PR ─▶ review ─▶ fix ─┐
                                          ▲            └──────────┘ (up to 3 rounds)
                                          └─ fix ◀─ PR gates fail   (up to 3 rounds)
                                                   gates green ─▶ agent:done
```

| Label | Meaning |
|---|---|
| `agent` | you ask for work (removed when the run starts) |
| `agent:doing` | implementing / reviewing / waiting for the gates |
| `agent:done` | PR is reviewed and green; your turn to review and merge |
| `agent:fail` | gave up; the reason is in an issue comment, details in the run logs |

Labels are mirrored on the PR. To ask for more work after reviewing, comment on the issue (or the PR,
or leave review comments) and add `agent` again: the run continues on the same branch `agent/issue-N`
and PR, with your comments (and only yours) as input.

The PR is assigned to you. GitHub does not let the PR author be a reviewer, so the review request only
takes effect if `AGENT_PAT` belongs to another account (e.g. a bot).

Guard rails: only `RafaelKC` adding the label starts a run, and that label is your approval of the
issue, even if someone else wrote it. Text from other authors goes into the prompt marked as
untrusted, and Claude runs without `GH_TOKEN` in its environment, so an instruction hidden in an issue
cannot use your PAT (the script does the pushing and the PR). Only your own comments are used as
instructions. The agent cannot change `.github/workflows/` or `scripts/agent/`; the implementer and the
reviewer are separate Claude sessions; the reviewer is read-only. Read an external author's issue
before labeling it.

## Setup on the runner (once)

Use a dedicated unprivileged user (`agent`) on the machine: the agent runs shell commands.

1. Packages: `sudo apt install build-essential git jq shellcheck curl`, the
   [GitHub CLI](https://cli.github.com), Rust via [rustup](https://rustup.rs) (`rustfmt` + `clippy`
   components) and Claude Code (`curl -fsSL https://claude.ai/install.sh | bash`).
2. Claude subscription login. On a machine with a browser run `claude setup-token`, then on the
   runner put `CLAUDE_CODE_OAUTH_TOKEN=<token>` in `~/actions-runner/.env`. (Or run `claude` once on the
   runner and log in.) Do **not** set `ANTHROPIC_API_KEY`; the script unsets it so usage stays on the
   subscription. Check with `claude -p "say ok"` as that user.
3. Runner: Settings → Actions → Runners → New self-hosted runner → Linux / ARM64, add the label
   `twig-agent`, and install it as a service (`sudo ./svc.sh install agent && sudo ./svc.sh start`).
4. `AGENT_PAT` secret (Settings → Secrets and variables → Actions): a fine-grained token for this repo
   only with Contents, Pull requests, Issues and Actions (read) = read/write. It must be a PAT, not the
   default token, otherwise `ci.yml` never runs on the agent's PRs.
5. Settings → Actions → General: "Require approval for all outside collaborators" for fork PR
   workflows. The self-hosted runner never runs on pull requests: `agent.yml` only listens to
   `issues: labeled`.
6. Optional but recommended: branch protection on `main` requiring the `check`, `test-macos` and
   `windows` jobs, so nothing merges red (including your own merge click).

## Tuning

Environment variables for `scripts/agent/run.sh` (set them in the workflow `env:`): `AGENT_MODEL`,
`AGENT_MAX_REVIEW_ROUNDS` (3), `AGENT_MAX_CI_ROUNDS` (3), `AGENT_CLAUDE_TIMEOUT` (45m per call),
`AGENT_CI_TIMEOUT` (5400 s). Logs of every Claude call are kept under `~/agent/logs/` on the runner;
the build cache is shared in `~/agent/target`.

## Not covered yet

Reacting to PR comments without re-adding the label, auto-merge, and a rerun when `main` moves
(only conflicts are handled, on the next run).
