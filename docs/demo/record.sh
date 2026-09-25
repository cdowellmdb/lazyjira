#!/usr/bin/env bash
# Re-records docs/images/demo.gif against fake Jira data. Requires vhs.
#
# lazyjira runs with a throwaway HOME and TMPDIR, and docs/demo/bin/jira
# stands in for the real jira CLI, so no real config, cache, or Jira
# data is read or shown.
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
demo_home="$(mktemp -d)"
trap 'rm -rf "$demo_home"' EXIT

mkdir -p "$demo_home/.config/lazyjira" "$demo_home/tmp"
cp "$repo/docs/demo/config.toml" "$demo_home/.config/lazyjira/config.toml"

cargo build --release --manifest-path "$repo/Cargo.toml"

cd "$repo"
LAZYJIRA_DEMO_HOME="$demo_home" \
  LAZYJIRA_DEMO_PATH="$repo/docs/demo/bin:$repo/target/release:$PATH" \
  vhs docs/demo/demo.tape
