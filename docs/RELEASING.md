# Releasing

## Cut a release

```bash
scripts/release.sh 0.2.0
```

The script, run from an up-to-date and clean `main`:

1. sets `version` in `Cargo.toml` (and `Cargo.lock`, via `cargo test`);
2. runs the tests;
3. commits `release v0.2.0` and creates the annotated tag `v0.2.0`;
4. asks before pushing `main` and the tag.

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
