#!/bin/sh

set -eu

context_cwd=''
if [ -n "${HERDR_PLUGIN_CONTEXT_JSON:-}" ]; then
    context_cwd=$(printf '%s\n' "$HERDR_PLUGIN_CONTEXT_JSON" \
        | sed -n 's/.*"focused_pane_cwd":"\([^"]*\)".*/\1/p')
fi
workspace_cwd=${context_cwd:-${HERDR_ACTIVE_PANE_CWD:-$PWD}}

set -- herdr plugin pane open \
    --plugin herdr-todo \
    --entrypoint todo \
    --placement split \
    --direction right \
    --cwd "$workspace_cwd" \
    --no-focus

if [ -n "${HERDR_PANE_ID:-}" ]; then
    set -- "$@" --target-pane "$HERDR_PANE_ID"
    set -- "$@" --env "HERDR_TODO_SOURCE_PANE_ID=$HERDR_PANE_ID"
fi

exec "$@"
