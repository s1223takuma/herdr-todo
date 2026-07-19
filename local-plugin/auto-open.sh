#!/bin/sh

set -eu

HERDR_CMD=${HERDR_BIN_PATH:-herdr}
EVENT_JSON=${HERDR_PLUGIN_EVENT_JSON:-}
PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${PATH:-}"
export PATH

[ -n "$EVENT_JSON" ] || exit 0

workspace_id=$(printf '%s\n' "$EVENT_JSON" \
    | jq -r '.data.workspace.workspace_id // .data.workspace_id // empty' 2>/dev/null)
[ -n "$workspace_id" ] || exit 0

panes_json=''
attempt=0
while [ "$attempt" -lt 10 ]; do
    panes_json=$($HERDR_CMD pane list --workspace "$workspace_id" 2>/dev/null || true)
    if printf '%s\n' "$panes_json" | jq -e '.result.panes[0].pane_id' >/dev/null 2>&1; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done

[ -n "$panes_json" ] || exit 0

# The TODO pane is renamed to "TODO" by its manifest. Avoid duplicates.
if printf '%s\n' "$panes_json" \
    | jq -e '.result.panes[] | select((.label // "") == "TODO" or ((.terminal_title_stripped // "") | contains("herdr-todo")))' \
        >/dev/null 2>&1; then
    exit 0
fi

source_pane_id=$(printf '%s\n' "$panes_json" | jq -r '.result.panes[0].pane_id // empty')
[ -n "$source_pane_id" ] || exit 0

source_cwd=$(printf '%s\n' "$panes_json" \
    | jq -r --arg pane "$source_pane_id" \
        '.result.panes[] | select(.pane_id == $pane) | .foreground_cwd // .cwd // empty')
[ -n "$source_cwd" ] || source_cwd=$PWD

exec "$HERDR_CMD" plugin pane open \
    --plugin herdr-todo \
    --entrypoint todo \
    --placement split \
    --direction right \
    --target-pane "$source_pane_id" \
    --cwd "$source_cwd" \
    --env "HERDR_TODO_SOURCE_PANE_ID=$source_pane_id" \
    --no-focus
