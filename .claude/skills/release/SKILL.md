---
name: release
description: Tag a new version, make a new release
---

# Release Process

The version bump goes through a PR. The tag is pushed only after the PR is
merged. Pushing a `v*` tag starts `.github/workflows/release.yml`, which builds
the binaries and creates the GitHub release. The release notes are the
`## [X.Y.Z]` section of `CHANGELOG.md`, so that section must be complete
before you tag.

1. Start from an up-to-date `main`: `git switch main && git pull`, then
   `git switch -c release-X.Y.Z`.
2. Bump `version` in `Cargo.toml`.
3. Update `CHANGELOG.md`: rename `## [Unreleased]` to
   `## [X.Y.Z] - YYYY-MM-DD` (today's date), and add a new empty
   `## [Unreleased]` heading above it. Check that the section covers every
   user-visible change since the previous tag (`git log vPREV..HEAD`).
4. Run the check sequence from `CLAUDE.md`:
   - `cargo fmt`
   - `cargo clippy` (must be clean; the known `--all-targets` warning in
     `src/output/compact.rs` is out of scope)
   - `cargo test` (all tests must pass)
   - `cargo build --release` (must succeed; this also updates `Cargo.lock`)
   - `cargo package --allow-dirty --target-dir target/package-verify` (the
     verification build must compile)
5. Commit `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` together with the
   message `Bump version to X.Y.Z`.
6. Push the branch and open a PR:
   `git push -u origin release-X.Y.Z && gh pr create --fill`.
   Wait for the user to merge it.
7. After the merge: `git switch main && git pull`, and check that `HEAD` is the
   merge commit containing the bump.
8. Tag that commit with a lightweight tag and push it:
   `git tag vX.Y.Z && git push origin vX.Y.Z`.
9. Watch the release workflow (`gh run watch`). Then check that the GitHub
   release exists with the changelog section as its body and all seven
   archives attached (`gh release view vX.Y.Z`).
10. Publish to crates.io from the tagged `main`:
    `cargo publish --target-dir target/package-verify` (the separate target
    dir keeps the verification build from leaving `target/debug` stale; see
    the packaging step in `CLAUDE.md`).
