#!/usr/bin/env bash
# Re-records docs/images/demo.gif (or the tape given, such as docs/demo/themes.tape)
# against fake Jira data. Requires vhs and python3.
#
# lazyjira runs with a throwaway HOME and TMPDIR. docs/demo/bin/jira stands in for the real
# jira CLI, and docs/demo/bin/fake_jira_rest.py for Jira's REST search, which jira-cli's
# config in the throwaway HOME points at, so no real config, cache, or Jira data is read or
# shown.
set -euo pipefail

repo="$(cd "$(dirname "$0")/../.." && pwd)"
demo_home="$(mktemp -d)"
rest_pid=""
trap '[ -z "$rest_pid" ] || kill "$rest_pid" 2>/dev/null || true; rm -rf "$demo_home"' EXIT

mkdir -p "$demo_home/.config/lazyjira" "$demo_home/.config/.jira" "$demo_home/tmp"
cp "$repo/docs/demo/config.toml" "$demo_home/.config/lazyjira/config.toml"

python3 "$repo/docs/demo/bin/fake_jira_rest.py" "$demo_home/rest-port" &
rest_pid=$!
for _ in $(seq 50); do
  [ -s "$demo_home/rest-port" ] && break
  sleep 0.1
done
if [ ! -s "$demo_home/rest-port" ]; then
  echo "The fake Jira REST server didn't start." >&2
  exit 1
fi

# `epic.link` is the Epic Link field the fake data uses (EPIC_LINK_FIELD in bin/jira).
cat > "$demo_home/.config/.jira/.config.yml" <<EOF
installation: Local
server: http://127.0.0.1:$(cat "$demo_home/rest-port")
auth_type: bearer
login: alex.rivera@example.com
project:
  key: DEMO
epic:
  name: customfield_10858
  link: customfield_10857
EOF

cargo build --release --manifest-path "$repo/Cargo.toml"

cd "$repo"
env -u JIRA_CONFIG_FILE -u NO_COLOR \
  JIRA_API_TOKEN=demo \
  LAZYJIRA_DEMO_HOME="$demo_home" \
  LAZYJIRA_DEMO_PATH="$repo/docs/demo/bin:$repo/target/release:$PATH" \
  vhs "${1:-docs/demo/demo.tape}"
