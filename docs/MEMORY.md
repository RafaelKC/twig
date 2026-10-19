# Card memory

> **In development.** `twig memo` is the first, deliberately simple version of
> [#16](https://github.com/RafaelKC/twig/issues/16). The command, the file format and the
> context-file section may change before they are stable. Use it, but do not build tooling
> that depends on the exact format yet.

## Why

An agent session ends and everything it learned about the card goes with it: what was done,
what is left, why a decision was taken, how to run the thing. The next session (or the next
agent, or you next week) starts from zero and re-derives it.

The card memory is a small, append-only log per card that survives sessions **and the card
itself**: when a card is closed with `twig rm` and QA asks for it back a month later, the
notes are still there.

## Layout

```
<worktrees>/                          (default ~/.wt)
├── 52644/                            the card
│   ├── CLAUDE.md                     generated; includes the last 10 memo entries
│   ├── MEMORY.md  ->  ../memos/52644-az/MEMORY.md     relative symlink
│   ├── api/                          worktrees
│   └── web/
└── memos/                            never a card; ignored by ls, status, sync, prune, ...
    └── 52644-az/
        └── MEMORY.md                 the real file
```

- The real file is `<worktrees>/memos/<id>-<provider>/MEMORY.md`. `<provider>` is the
  configured provider (`az` or `gh`), so ids from different providers never collide.
- The card folder only holds a **relative** symlink to it, so the agent can simply read
  `MEMORY.md` in the card root, and moving the whole worktrees folder keeps working.
- `memos` is a reserved name: it is never listed as a card, `twig new memos` is refused, and
  a working directory inside `memos/` does not infer a card.
- Symlinks are Unix-only. If the link cannot be created, twig warns and prints where the real
  file is; nothing crashes.

## Lifecycle

| Event | What happens to the memo |
|---|---|
| `twig new <id>` | creates `memos/<id>-<provider>/MEMORY.md` if missing and links it. If a memo already exists (a reopened card), the link is restored and twig says so. |
| `twig add-repo`, `twig refresh` | make sure the link exists and regenerate the context file with the last entries |
| `twig memo "text"` | appends one line at the end of the file |
| `twig rm <id>` | removes the card folder and the symlink; the memo is **kept** and `rm` prints where |
| `twig prune` | never touches `memos/` |
| `twig new <id>` again (e.g. QA reopens the card) | finds the memo, recreates the symlink, and the next context file shows its last entries |

## File format

Plain text, one entry per line, oldest first:

```
2026-10-19 14:32: moved validation to the API layer; web only shows the error
2026-10-19 16:05: tests green in api, web still needs the e2e for the empty state
```

- `YYYY-MM-DD HH:MM: TEXT`, **local time**.
- Newlines (and runs of whitespace) in the text collapse to single spaces, so an entry is
  always exactly one line.
- You can edit the file by hand (it is just markdown/text); keep one entry per line.

## What goes into the context file

On `new`, `add-repo` and `refresh`, the generated context file (`CLAUDE.md` / `AGENTS.md`)
gets a section before "Card context":

```
## Memory (last 10 entries, full file: MEMORY.md)
```

with the last `MEMO_CONTEXT_ENTRIES` (= 10, a constant in `src/memo.rs`) lines, newest last.
The section is omitted when there is no memo or it is empty. The full history stays in
`MEMORY.md`.

## How agents use it

The twig skill (`skills/twig/SKILL.md`) tells agents to:

1. read `MEMORY.md` when they enter a card;
2. run `twig memo "..."` after finishing a chunk of work, after making a decision and before
   stopping, one line each: what was done, what is left, the decision and why.

## Not done yet

- **Consolidation:** the file only grows; nothing summarizes old entries.
- **Size cap:** no limit on the file size (only the last 10 entries reach the context file).
- **Export on `rm`:** copying the memo somewhere else (a note, the issue, a PR comment) when
  the card is removed, through lifecycle hooks ([#15](https://github.com/RafaelKC/twig/issues/15)).
- **Structured sections:** decisions, open questions and how-to-run as separate parts instead
  of a flat log.
- **Windows:** needs symlink support; Unix only for now.
- The exact format and command surface may change until this is declared stable.

See also: [COMMANDS.md](COMMANDS.md), [ROADMAP.md](ROADMAP.md).
