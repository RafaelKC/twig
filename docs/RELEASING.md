# Releasing

How the workflows, jobs, scripts and agents fit together, from issue to release: [PIPELINE.md](PIPELINE.md).

## Cut a release

`main` only accepts changes through a pull request, so a release takes two steps.

```bash
scripts/release.sh 0.2.0          # 1. open the release PR
# wait for `check`, `test-macos` and `windows`, then merge the PR
git switch main && git pull
scripts/release.sh --tag 0.2.0    # 2. tag the merged release commit
```

Step 1, run from an up-to-date and clean `main`:

1. creates the branch `release/v0.2.0` (never `v0.2.0`: a branch named like the tag collided with it once);
2. sets `version` in `Cargo.toml` (and `Cargo.lock`, via `cargo test`);
3. runs the tests;
4. commits `release v0.2.0`;
5. asks before pushing the branch and opening the PR with `gh pr create`.

Step 2 (`--tag`), run from an up-to-date and clean `main`:

1. checks `main` contains the `release v0.2.0` commit (a squash-merge `(#NN)` suffix is fine) with that version in `Cargo.toml`;
2. creates the annotated tag `v0.2.0` on that commit;
3. asks before pushing **only the tag**.

Both steps refuse to run if the tag already exists locally or on `origin`.

## Cut a release from GitHub Actions

The same flow without a local checkout: Actions > **release-prepare** > Run workflow, branch
`main`, version `0.2.0`.

1. [`release-prepare.yml`](../.github/workflows/release-prepare.yml) bumps the version on
   `release/v0.2.0`, runs the tests, opens the PR `release v0.2.0` and turns on auto-merge (squash);
2. once `check`, `test-macos` and `windows` pass, the PR merges;
3. [`release-tag.yml`](../.github/workflows/release-tag.yml) sees the `release v0.2.0` commit on
   `main`, checks `Cargo.toml`, creates the annotated tag `v0.2.0` and pushes it, which starts
   `release.yml`.

Only `RafaelKC` can run it: both workflows skip their job for any other actor, and
`release-prepare` only runs from `main`. They use the `AGENT_PAT` secret, because pushes, PRs and
tags made with `github.token` don't trigger other workflows (the CI checks and `release.yml`).
Auto-merge must be allowed in Settings > General; without it the PR stays open and is merged by
hand, and the tag still follows the merge.

## Branch rules for `main`

Recommended ruleset (Settings > Rules > Rulesets), configured by hand in GitHub:

- **Branch `main`**: require a pull request before merging; require the status checks
  `check`, `test-macos` and `windows` (from [`ci.yml`](../.github/workflows/ci.yml)), with the branch up to date;
  block force pushes and deletion.
- **Tags `v*`**: restrict updates and deletion, so a published release tag can't be moved or
  re-created. Tag creation must stay allowed for whoever runs `--tag`.

The release flow needs no bypass: the version bump goes through a PR and only the tag is pushed
directly. The release workflow runs on the `v*` tag push, so these rules don't affect it.

## What the tag does

The tag triggers [`.github/workflows/release.yml`](../.github/workflows/release.yml):

| Job | What it does |
|---|---|
| `test` | checks the tag matches `Cargo.toml` and runs `cargo test --locked` |
| `build` | builds and packages each target (below) |
| `notes` | writes the release description with Claude (best effort, see below) |
| `release` | `SHA256SUMS`, Sigstore signature, build provenance, GitHub Release with the notes |
| `changelog` | adds the release notes to `CHANGELOG.md` through an auto-merged PR (best effort, see below) |

| Target | Runner | Notes |
|---|---|---|
| `x86_64-unknown-linux-musl` | `ubuntu-24.04` | static binary, any distro / WSL |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | static binary |
| `x86_64-apple-darwin` | `macos-14` | Intel Macs |
| `aarch64-apple-darwin` | `macos-14` | Apple Silicon |
| `x86_64-pc-windows-msvc` | `windows-2025` | native Windows (beta, like herdr's); also runs on Windows on ARM under emulation |

## Release notes

The description is written by Claude from the PRs merged into `main` and the issues closed since
the previous tag (the `release vX.Y.Z` PR is left out). It has a short summary and the sections
Highlights, Improvements, Fixes and Breaking changes (only those with content), each item with its
`#N`. GitHub then appends the full PR list and the `Full Changelog` link
(`generate_release_notes`).

- **Runner**: the `notes` job runs on the agent's self-hosted runner and reuses its Claude login
  (see [AGENT.md](AGENT.md)); no API key. If `claude` is missing, fails or takes more than 5
  minutes, the release goes out with GitHub's generated notes only. If the runner is offline the
  job waits in the queue and so does the release: bring the runner up (or cancel and re-run).
- **Safety**: PR and issue text is untrusted input. The `notes` job has a read-only token and
  `claude` runs with no tools, so it only turns text into markdown.
- **Prompt**: [`scripts/release/notes-prompt.md`](../scripts/release/notes-prompt.md).
- **Preview locally** (needs `gh`, `jq` and `claude`):

  ```bash
  scripts/release/notes.sh v0.7.0 v0.8.0 > notes.md   # previous tag, new tag (default: HEAD)
  NOTES_DRY=1 scripts/release/notes.sh v0.7.0 v0.8.0  # only the prompt and the collected data
  ```

- **Edit after publishing**: `gh release edit vX.Y.Z --notes-file notes.md` (the generated PR list
  and changelog link at the end are part of the text, keep them).

## CHANGELOG.md

Once the release is published, the `changelog` job runs
[`scripts/release/changelog.sh`](../scripts/release/changelog.sh) on the release description: the
notes written by Claude become the `## [X.Y.Z] - date` entry (their headings one level down), and
GitHub's generated PR list and `Full Changelog` link are dropped. When there are no Claude notes,
the PR list becomes the entry. The compare link is added at the end of the file.

`main` only accepts PRs, so the job opens `docs: changelog for vX.Y.Z` on `docs/changelog-vX.Y.Z`
with the `AGENT_PAT` secret and turns on auto-merge. A version already in the file is skipped. If the
job fails the release is still out; add the entry by hand:

```bash
gh release view v0.9.1 --json body --jq .body | scripts/release/changelog.sh v0.9.1 2026-10-29
```

Notes edited after publishing aren't picked up again: edit `CHANGELOG.md` too.

## Release assets

| File | Content |
|---|---|
| `twig-<target>.tar.gz` | `twig-<target>/` with the `twig` binary, `skills/twig`, `herdr-plugins/twig`, `LICENSE`, `README.md` |
| `twig-x86_64-pc-windows-msvc.zip` | the same layout with `twig.exe` |
| `install.sh` | the installer at that version |
| `install.ps1` | the Windows installer at that version (`releases/latest/download/install.ps1` is the latest one; `https://rndm.chicovis.dev/twig/windows-install` redirects to it) |
| `SHA256SUMS` | checksums of the archives, `install.sh` and `install.ps1` |
| `SHA256SUMS.sigstore.json` | Sigstore bundle signing `SHA256SUMS` |

## Signing and verification

Signing is keyless: the workflow signs with its own GitHub OIDC identity through
[Sigstore](https://www.sigstore.dev), so there's no key to store or leak. The signature
proves `SHA256SUMS` was produced by `release.yml` in this repository for a `v*` tag, and
the checksums cover every archive.

The installer always checks the checksum, and checks the signature too when `cosign` is
installed. To verify by hand:

```bash
cosign verify-blob SHA256SUMS \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github.com/RafaelKC/twig/\.github/workflows/release\.yml@refs/tags/' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum --ignore-missing -c SHA256SUMS
```

Each archive also has a build provenance attestation:

```bash
gh attestation verify twig-x86_64-unknown-linux-musl.tar.gz --repo RafaelKC/twig
```

On Windows, `install.ps1` checks the checksum with `Get-FileHash` (and the signature when
`cosign` is on the PATH). `twig.exe` has no Authenticode signature yet: downloaded through a
browser, SmartScreen may warn on the first run; the installer's download isn't flagged.

macOS binaries are not signed or notarized with an Apple Developer ID. Files downloaded
with `curl` (as the installer does) don't get the quarantine flag, so Gatekeeper doesn't
block them; a file downloaded through a browser would need
`xattr -d com.apple.quarantine twig`.
