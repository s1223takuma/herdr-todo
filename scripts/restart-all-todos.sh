#!/bin/sh

set -eu

HERDR_CMD=${HERDR_BIN_PATH:-herdr}
PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${PATH:-}"
export PATH

if ! command -v jq >/dev/null 2>&1; then
    printf '%s\n' "Restart All TODOs requires jq" >&2
    exit 1
fi

SCRIPT_DIR=$(CDPATH= cd "$(dirname "$0")" && pwd)
panes_json=$($HERDR_CMD pane list)
restart_rows=$(printf '%s\n' "$panes_json" | jq -r '
    .result.panes
    | group_by(.workspace_id)[]
    | ([.[] | select((.label // "") == "TODO")] ) as $todos
    | ([.[] | select((.label // "") != "TODO")][0]) as $source
    | select(($todos | length) > 0 and $source != null)
    | [
        $source.pane_id,
        ($source.foreground_cwd // $source.cwd // "."),
        ($todos | map(.pane_id) | join(","))
      ]
    | @tsv
')

[ -n "$restart_rows" ] || exit 0

printf '%s\n' "$restart_rows" |
while IFS="$(printf '\t')" read -r source_pane_id source_cwd todo_pane_ids; do
    old_ifs=$IFS
    IFS=','
    for todo_pane_id in $todo_pane_ids; do
        $HERDR_CMD plugin pane close "$todo_pane_id" >/dev/null 2>&1 || \
            $HERDR_CMD pane close "$todo_pane_id" >/dev/null 2>&1 || true
    done
    IFS=$old_ifs

    HERDR_PANE_ID=$source_pane_id \
    HERDR_ACTIVE_PANE_CWD=$source_cwd \
    HERDR_PLUGIN_CONTEXT_JSON='' \
    HERDR_BIN_PATH=$HERDR_CMD \
        sh "$SCRIPT_DIR/open-todo.sh" >/dev/null
done
