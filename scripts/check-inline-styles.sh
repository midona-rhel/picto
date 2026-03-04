#!/bin/bash
# PBI-052: Check for inline style literals that should use design tokens.
# Flags literal color/font/border/background values in style={{ ... }} blocks.
# Dynamic geometry (width, height, top, left, etc.) is allowed.
#
# Usage: ./scripts/check-inline-styles.sh [--ci]
# Exit code 1 if violations found (for CI gating).

AUDITED_PATHS=(
  "src/components/image-grid"
  "src/components/sidebar"
  "src/components/layout"
  "src/components/settings"
)

# Patterns that indicate non-token color/font usage in TSX inline styles.
# These are heuristic — suppress false positives with // pbi-052-suppress comment.
PATTERNS=(
  "color: ['\"]#[0-9a-fA-F]"
  "color: ['\"]rgba\\("
  "background: ['\"]#[0-9a-fA-F]"
  "background: ['\"]rgba\\("
  "fontSize: [0-9]"
  "fontWeight: [0-9]"
  "borderColor: ['\"]#"
)

FOUND=0

for dir in "${AUDITED_PATHS[@]}"; do
  for pattern in "${PATTERNS[@]}"; do
    matches=$(grep -rn --include="*.tsx" "$pattern" "$dir" 2>/dev/null | grep -v "pbi-052-suppress" | grep -v "__tests__" | grep -v ".test." || true)
    if [ -n "$matches" ]; then
      if [ "$FOUND" -eq 0 ]; then
        echo "PBI-052: Found inline style literals that should use design tokens:"
        echo "========================================================================="
      fi
      echo "$matches"
      FOUND=1
    fi
  done
done

if [ "$FOUND" -eq 0 ]; then
  echo "PBI-052: No inline style violations found in audited paths."
  exit 0
else
  echo ""
  echo "Fix: Replace literal values with CSS variables or move to CSS modules."
  echo "Suppress: Add '// pbi-052-suppress' comment for justified dynamic values."
  if [ "$1" = "--ci" ]; then
    exit 1
  fi
  exit 0
fi
