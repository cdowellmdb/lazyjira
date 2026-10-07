# Releasing lazyjira

`lazyjira` now uses GitHub Releases for versioned builds.

## Normal Release Flow

1. Make sure the changes you want are merged to `main`.
2. Bump `version` in `Cargo.toml`, and the `--tag` in README.md's install command, to match the new tag (`cargo test` checks the two agree).
3. Run `cargo check` so `Cargo.lock` picks up the new version. The release workflow builds with `--locked` and fails if the lockfile is stale.
4. Update the `--tag` in README.md's install command to the new tag.
5. Commit the version bump on `main`.
6. Create an annotated tag like `v0.1.2`.
7. Push the tag to GitHub.

Example:

```bash
git switch main
git pull --ff-only
$EDITOR Cargo.toml README.md
cargo check
git commit -am "chore(release): bump version to 0.1.2"
git tag -a v0.1.2 -m "v0.1.2"
git push origin main
git push origin v0.1.2
```

## What Happens After Tag Push

Pushing a `v*` tag triggers `.github/workflows/release.yml`.

That workflow:

- builds `lazyjira` in release mode
- packages binaries for Linux x86_64, macOS Intel, and macOS Apple Silicon
- creates or updates the matching GitHub Release
- attaches the built archives as release assets
- generates GitHub release notes automatically

## Installing A Released Build

From a release asset:

1. Download the archive for your platform from the GitHub Release page.
2. Extract it.
3. Move the `lazyjira` binary somewhere on your `PATH`, such as `~/.cargo/bin/`.

From source at a release tag:

```bash
cargo install --git https://github.com/cdowellmdb/lazyjira --tag v0.1.1
```
