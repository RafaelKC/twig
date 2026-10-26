# Park marker (`.parked.json`)

`twig park <id>` writes `<worktrees>/<id>/.parked.json`. It records that the card was parked
on purpose (waiting for QA, review...), since when, and what the card looked like then, so
`twig ls`/`twig status` and external tools can tell it from a card that merely has its herdr
workspace closed. The format is a **stable contract**: it is generic, nothing in it depends on
a specific provider or company.

```json
{"v":1,"since":"2026-10-26T14:03:00-03:00","state":"Resolved","children":[51234,51240]}
```

| Field | Type | Meaning |
|---|---|---|
| `v` | number | Format version (`1`). Readers should ignore a version they don't know. |
| `since` | string | Local date and time of the park, ISO 8601 with offset. |
| `state` | string \| `null` | The card's state in the provider at park time (`CardInfo.state`); `null` when it couldn't be obtained. |
| `children` | array \| `null` | Ids of the card's child items at park time (numbers when the id is numeric, strings otherwise); `null` when the provider doesn't support it or is unavailable. |

Adding fields keeps `v` at 1; removing or changing one bumps it.

## Where the data comes from

Best effort, with no flag: twig asks the provider again (`az` / `gh`), falls back to the card
cache (`.ado.json` / `.gh.json`) and, with neither, writes `null` and warns.

- **az:** `state` is `System.State`; `children` are the `System.LinkTypes.Hierarchy-Forward`
  relations of the work item.
- **gh:** `state` is the issue state; `children` are the issue's sub-issues (`subIssues` in
  `gh issue view --json`). With a `gh` too old to know `subIssues`, or a cache saved by one,
  `children` is `null`.

## Lifecycle

- Parking an already parked card **keeps the original marker** (`since` and `children` are the
  baseline to compare against) and only reports since when it has been parked.
- `twig park <id> --undo` (or `twig unpark <id>`) deletes the marker without opening or
  closing anything.
- `twig open` / `twig herdr` do **not** remove it: working on the card again doesn't mean it
  left the wait. Only `--undo` does.
- `twig rm` removes it with the card folder.
- `twig ls --json` and `twig status --json` expose `since` as `parked_since` (`null` when not parked).
