#!/bin/sh

set -eu

HERDR_CMD=${HERDR_BIN_PATH:-herdr}
PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${PATH:-}"
export PATH

context_cwd=''
context_pane_id=''
if [ -n "${HERDR_PLUGIN_CONTEXT_JSON:-}" ]; then
    if command -v jq >/dev/null 2>&1; then
        context_cwd=$(printf '%s\n' "$HERDR_PLUGIN_CONTEXT_JSON" \
            | jq -r '.focused_pane_cwd // empty' 2>/dev/null)
        context_pane_id=$(printf '%s\n' "$HERDR_PLUGIN_CONTEXT_JSON" \
            | jq -r '.focused_pane_id // empty' 2>/dev/null)
    else
        context_cwd=$(printf '%s\n' "$HERDR_PLUGIN_CONTEXT_JSON" \
            | sed -n 's/.*"focused_pane_cwd":"\([^"]*\)".*/\1/p')
        context_pane_id=$(printf '%s\n' "$HERDR_PLUGIN_CONTEXT_JSON" \
            | sed -n 's/.*"focused_pane_id":"\([^"]*\)".*/\1/p')
    fi
fi
workspace_cwd=${context_cwd:-${HERDR_ACTIVE_PANE_CWD:-$PWD}}
source_pane_id=${HERDR_PANE_ID:-$context_pane_id}
target_pane_id=$source_pane_id

# Match the auto-open layout used for new workspaces. Splitting the workspace's
# first pane avoids inheriting the width of a nested/focused split.
if [ -n "$source_pane_id" ] && command -v jq >/dev/null 2>&1; then
    pane_json=$($HERDR_CMD pane get "$source_pane_id" 2>/dev/null || true)
    workspace_id=$(printf '%s\n' "$pane_json" \
        | jq -r '.result.pane.workspace_id // .result.workspace_id // empty' 2>/dev/null)
    if [ -n "$workspace_id" ]; then
        panes_json=$($HERDR_CMD pane list --workspace "$workspace_id" 2>/dev/null || true)
        workspace_root_pane=$(printf '%s\n' "$panes_json" \
            | jq -r '.result.panes[0].pane_id // empty' 2>/dev/null)
        [ -z "$workspace_root_pane" ] || target_pane_id=$workspace_root_pane
    fi
fi

set -- "$HERDR_CMD" plugin pane open \
    --plugin herdr-todo \
    --entrypoint todo \
    --placement split \
    --direction right \
    --cwd "$workspace_cwd" \
    --no-focus

if [ -n "$target_pane_id" ]; then
    set -- "$@" --target-pane "$target_pane_id"
fi

if [ -n "$source_pane_id" ]; then
    set -- "$@" --env "HERDR_TODO_SOURCE_PANE_ID=$source_pane_id"
fi

open_output=$("$@")
printf '%s\n' "$open_output"

# Plugin split defaults can vary depending on when the
# action runs. Normalize normal launches to the auto-open workspace ratio.
if command -v jq >/dev/null 2>&1; then
    todo_pane_id=$(printf '%s\n' "$open_output" \
        | jq -r '.result.plugin_pane.pane.pane_id // empty' 2>/dev/null)
    if [ -n "$todo_pane_id" ]; then
        layout_json=$($HERDR_CMD pane layout --pane "$todo_pane_id" 2>/dev/null || true)
        current_ratio=$(printf '%s\n' "$layout_json" \
            | jq -r '(.result.layout.splits[] | select(.id == "split_0_root") | .ratio) // empty' \
                2>/dev/null)
        if [ -n "$current_ratio" ]; then
            adjustment=$(awk -v current="$current_ratio" 'BEGIN {
                delta = 0.65 - current
                if (delta > 0.001) printf "right %.6f", delta
                else if (delta < -0.001) printf "left %.6f", -delta
            }')
            if [ -n "$adjustment" ]; then
                set -- $adjustment
                $HERDR_CMD pane resize \
                    --direction "$1" \
                    --amount "$2" \
                    --pane "$todo_pane_id" >/dev/null 2>&1 || true
            fi
        fi
    fi
fi
