# Releasing

## Cut a release

`main` only accepts changes through a pull request, so a release takes two steps.

```bash
scripts/release.sh 0.2.0          # 1. open the release PR
# wait for `check` and `test-macos`, then merge the PR
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

## Branch rules for `main`

Recommended ruleset (Settings > Rules > Rulesets), configured by hand in GitHub:

- **Branch `main`**: require a pull request before merging; require the status checks
  `check` and `test-macos` (from [`ci.yml`](../.github/workflows/ci.yml)), with the branch up to date;
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
| `release` | `SHA256SUMS`, Sigstore signature, build provenance, GitHub Release with generated notes |

| Target | Runner | Notes |
|---|---|---|
| `x86_64-unknown-linux-musl` | `ubuntu-24.04` | static binary, any distro / WSL |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | static binary |
| `x86_64-apple-darwin` | `macos-14` | Intel Macs |
| `aarch64-apple-darwin` | `macos-14` | Apple Silicon |

## Release assets

| File | Content |
|---|---|
| `twig-<target>.tar.gz` | `twig-<target>/` with the `twig` binary, `skills/twig`, `herdr-plugins/twig`, `LICENSE`, `README.md` |
| `install.sh` | the installer at that version |
| `SHA256SUMS` | checksums of the archives and `install.sh` |
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

macOS binaries are not signed or notarized with an Apple Developer ID. Files downloaded
with `curl` (as the installer does) don't get the quarantine flag, so Gatekeeper doesn't
block them; a file downloaded through a browser would need
`xattr -d com.apple.quarantine twig`.
