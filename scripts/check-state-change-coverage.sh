#!/usr/bin/env bash
# Gate: every write command handler in the dispatch table must either:
# 1. Call emit_state_changed (state-changing command), OR
# 2. Have a "Task-only" or "Read-only" doc comment (classified as non-emitting)
#
# This prevents adding new write commands that silently skip state-change emission.

set -euo pipefail

DISPATCH="core/src/dispatch/mod.rs"
WRITE_COMMANDS=$(grep -oP '"[a-z_]+"' "$DISPATCH" | tr -d '"' | sort -u)

ERRORS=()

for cmd in $WRITE_COMMANDS; do
  # Find the handler file by grepping for the function name
  HANDLER_FILE=$(grep -rl "pub async fn ${cmd}\b" core/src/dispatch/typed/ 2>/dev/null | head -1)
  if [ -z "$HANDLER_FILE" ]; then
    continue # Not all dispatch entries are direct function names
  fi

  # Check if the handler emits state_changed OR has a task-only/read-only classification
  HAS_EMIT=$(grep -A30 "pub async fn ${cmd}\b" "$HANDLER_FILE" | grep -c "emit_state_changed" || true)
  HAS_CLASSIFICATION=$(grep -B3 "pub async fn ${cmd}\b" "$HANDLER_FILE" | grep -ciE "task-only|read-only" || true)

  if [ "$HAS_EMIT" -eq 0 ] && [ "$HAS_CLASSIFICATION" -eq 0 ]; then
    ERRORS+=("$HANDLER_FILE: $cmd — no emit_state_changed and no task-only/read-only classification")
  fi
done

if [ ${#ERRORS[@]} -gt 0 ]; then
  echo "State-change coverage check FAILED:"
  echo ""
  for err in "${ERRORS[@]}"; do
    echo "  - $err"
  done
  exit 1
fi

echo "State-change coverage check passed."
