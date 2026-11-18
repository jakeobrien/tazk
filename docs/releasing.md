# Releasing

Releases are built by GitHub Actions and published as GitHub Releases.

## Cutting a release

1. Bump `version` in `Cargo.toml` (and let `Cargo.lock` update).
2. Add a `## [X.Y.Z] - YYYY-MM-DD` section to `CHANGELOG.md`. Release notes are written by hand; this section becomes the GitHub Release body.
3. Commit and push to `main`; wait for CI to pass.
4. Tag and push the tag:

   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

The `Release` workflow (`.github/workflows/release.yml`) then:

1. **verify**: fails unless the tag is `vX.Y.Z` or `vX.Y.Z-rc.N`, `X.Y.Z` equals the `Cargo.toml` version and `CHANGELOG.md` has a section for it. Tags with a hyphen (`v0.9.0-rc.1`) are published as prereleases.
2. **build**: on native runners, for each target, runs `make test`, builds `cargo build --release --locked`, smoke-tests the binary and packages it.
3. **release**: writes `SHA256SUMS` and creates the GitHub Release with the archives, `SHA256SUMS` and `install.sh`. Nothing is published unless every build and test passed.

If a release fails part-way, fix the problem, delete the tag (`git push --delete origin vX.Y.Z`; `git tag -d vX.Y.Z`) and tag again. If the run got as far as the **release** job, a GitHub Release exists too; see [Cleaning up](#cleaning-up-a-release-or-rc).

## Release candidates

Use a release candidate to rehearse the pipeline or let people try a build before the final release. RCs go through exactly the same workflow as a normal release.

**Naming:** `vX.Y.Z-rc.N`, for example `v0.9.0-rc.1`, `v0.9.0-rc.2`. Keep the dot before `N`: semver compares the suffix piece by piece, so `rc.10` sorts after `rc.2`, but `rc10` sorts before `rc2`. Only the `-rc.N` suffix is accepted; any other tag shape fails verification.

**What differs from a final release:**

- `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` keep the plain version (`0.9.0`, `## [0.9.0]`). Verify strips the `-rc.N` suffix from the tag before comparing, so cutting an RC needs no version bump commit.
- The binary reports the plain version, so `tz --version` on an RC prints `0.9.0`, the same as the final. Use the release page or the commit to tell them apart.
- The GitHub Release is marked **Pre-release**, so it is never what `releases/latest` points to. A plain `curl ... | sh` keeps installing the newest *final* release and ignores RCs.

**Trying an RC:** pin it explicitly, since the default install won't find it:

```bash
curl -fsSL https://github.com/jakeobrien/tazk/releases/latest/download/install.sh | sh -s -- --version v0.9.0-rc.1
```

(Once at least one final release exists, `latest/download/install.sh` serves the newest final release's script, which is fine for installing an RC. Before the first final release, use the RC's own copy: `https://github.com/jakeobrien/tazk/releases/download/v0.9.0-rc.1/install.sh`.)

**Flow:**

1. Make sure `Cargo.toml` has `version = "X.Y.Z"` and `CHANGELOG.md` has a `## [X.Y.Z]` section, commit, push to `main`, wait for CI.
2. `git tag vX.Y.Z-rc.1 && git push origin vX.Y.Z-rc.1`
3. If the RC needs fixes, commit them and tag `vX.Y.Z-rc.2`. Never move or reuse an RC tag that has been published.
4. For the final release, tag `vX.Y.Z` on the commit you want to ship, as described above.

## Cleaning up a release or RC

A published RC can simply be left in place; it is labelled as a prerelease and does no harm. Delete it when it was only a rehearsal or when you want a tidy releases page after the final ships.

- **Release exists (the run reached the publish job):** delete the release and its tag together, then the local tag:

  ```bash
  gh release delete vX.Y.Z-rc.1 --cleanup-tag --yes
  git tag -d vX.Y.Z-rc.1
  ```

- **Run failed before publishing (only a tag exists):**

  ```bash
  git push --delete origin vX.Y.Z-rc.1
  git tag -d vX.Y.Z-rc.1
  ```

- **Retagging a deleted version:** after cleanup you can tag the same name again on a fixed commit. This is fine for an RC or release that nobody installed. Anyone who already installed it keeps the old binary, so once a version has been shared, bump to the next `-rc.N` instead.

Deleting a release removes its assets, so the install script can no longer fetch that version. `latest` is unaffected, as it only ever points at the newest non-prerelease.

## Targets

| Target | Runner | Notes |
|---|---|---|
| `aarch64-apple-darwin` | `macos-15` | macOS on Apple Silicon |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | statically linked; needs `musl-tools` (a dependency, `onig_sys`, compiles C code) |

Archives are named `tazk-<target>.tar.gz` (no version in the name, so `releases/latest/download/<name>` works) and contain a `tazk-<target>/` directory with `tz`, `LICENSE.md` and `README.md`.

## Install script

`install.sh` (repo root) is attached to every release, so `https://github.com/jakeobrien/tazk/releases/latest/download/install.sh` is always the script from the newest release. It detects the platform, downloads the archive and `SHA256SUMS` from the chosen release, verifies the checksum and installs `tz`. Options are documented in the README. It is tested by `tests/install_script.sh` against a fake release tree and linted with `shellcheck` in CI.

## Toolchain

`rust-toolchain.toml` pins the exact compiler (currently 1.100.0) used locally, in CI and for releases. `rust-version` in `Cargo.toml` is the minimum that builds and tests everything (1.96). Bump the pin deliberately; raise `rust-version` only when the code actually needs it.
