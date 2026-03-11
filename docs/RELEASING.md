# Releasing lazyjira

`lazyjira` now uses GitHub Releases for versioned builds.

## Normal Release Flow

1. Make sure the changes you want are merged to `main`.
2. Update `version` in `Cargo.toml` if you want the crate metadata to match the release tag.
3. Commit that version bump on `main`.
4. Create an annotated tag like `v0.1.1`.
5. Push the tag to GitHub.

Example:

```bash
git switch main
git pull --ff-only
$EDITOR Cargo.toml
git commit -am "chore(release): bump version to 0.1.1"
git tag -a v0.1.1 -m "v0.1.1"
git push origin main
git push origin v0.1.1
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

## First Release Recommendation

Start with a `v0.1.0` or `v0.1.1` tag on `main`, depending on whether you want to treat the current repo state as the initial release or the first patch after that.
