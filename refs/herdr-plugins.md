# References: herdr plugins (not used)

Plugins and projects evaluated but **not installed**. The ones that ship with twig are in
[herdr-plugins/](../herdr-plugins/README.md).

## Candidates (not installed)

Marketplace triage (`topic:herdr-plugin`) on 2026-10-15. Nothing here has been reviewed yet.

| Plugin | What for | Caveats |
|---|---|---|
| [persiyanov/herdr-reviewr](https://github.com/persiyanov/herdr-reviewr) | review the agent's diff and PRs (reads ADO PRs via `az`), comment per line and send to the agent; see `docs/ROADMAP.md` → PR review | herdr ≥ 0.9.3; build runs `install.sh` (prebuilt binary); use `auto_open = false` |
| [hhdebb/herdr-radar](https://github.com/hhdebb/herdr-radar) | agent sidebar with icons, "waiting on you", grouped by project/worktree | Node 18+, untested on Linux, writes to `config.toml`, logos may not render in Windows Terminal, may conflict with card status in the sidebar |
| [senna-lang/herdr-agent-usage](https://github.com/senna-lang/herdr-agent-usage) | context usage and rate limit per agent | build downloads/compiles a binary |
| [smarzban/herdr-file-viewer](https://github.com/smarzban/herdr-file-viewer) / [alexarthurs/herdr-sidebar](https://github.com/alexarthurs/herdr-sidebar) | file tree + git diff inside herdr (at most one of the two) | prebuilt binary; overlaps with VS Code |
| [nicosuave/memex](https://github.com/nicosuave/memex) | search Claude transcripts and resume sessions | — |
| [speardragon/herdr-plugin-manager](https://github.com/speardragon/herdr-plugin-manager) | plugin management popup | this file remains the source of truth |

Idea references for twig (don't install, they compete with `twig herdr`):
[herdr-sessionizer](https://github.com/andrewchng/herdr-sessionizer) (TOML layout + per-repo override),
[herdr-spreader](https://github.com/yuk1ty/herdr-spreader),
[herdr-plugin-workspace-manager](https://github.com/razajamil/herdr-plugin-workspace-manager) (layout on `worktree.created`),
[herdr-plus](https://github.com/cloudmanic/herdr-plus) (Projects / Quick Actions).

## Review notes

### herdr-bar — tested and removed on 2026-10-15

Removed: the goal was agent icons in the sidebar, and that's `herdr-radar`'s job
(`hhdebb/herdr-radar`); herdr-bar only shows icons in the search popup and,
optionally, in tab names. Review notes kept for reference:

- Python 3 with the standard library only, no build and no dependencies.
- No network access. Uses only the herdr socket/CLI, `ps` (uptime) and `fc-match` (icon font).
- Reads Claude/Codex transcripts in `~/.claude` **locally** to generate tab titles.
- Only renames **unnamed** tabs. Named tabs (`agent`, `run` from `twig herdr`) are left alone.
- Runs a background watcher (startup hook + `pane.agent_detected` event). To turn it off: `herdr plugin action invoke herdr-bar.stop-titles` or `"auto_titles": false` in `$(herdr plugin config-dir herdr-bar)/config.json`.
- The README's suggested shortcut is `prefix+k`, but it conflicts with `focus_pane_up`, so I used `prefix+space`.
- Updating: `herdr plugin update herdr-bar` re-fetches the pinned ref. To bump the version, review the diff, reinstall with the new `--ref` and update this table.
