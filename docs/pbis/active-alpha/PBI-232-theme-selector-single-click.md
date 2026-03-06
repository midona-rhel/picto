# PBI-232: Theme selector requires double-click instead of single-click

## Priority
P2

## Audit Status (2026-03-06)
Status: **Not Implemented**

Evidence:
1. A user reported: "theme selector seems a lil bugged" with a screenshot showing the theme picker.
2. Confirmed: "Yeah I noticed, just double click."
3. Theme options should respond to a single click but currently require a double-click to apply.

## Problem
The theme selector control requires a double-click to apply a theme change. Users expect a single click to select and apply the theme immediately.

## Scope
- Settings / theme selector component — fix click handler binding

## Implementation
1. Locate the theme selector component in settings.
2. Check if the click handler is bound to `onDoubleClick` instead of `onClick`, or if a competing event (e.g. focus, blur, or a parent handler) is swallowing the first click.
3. Fix the handler so a single click applies the selected theme.
4. Ensure the active theme is visually indicated (highlight, checkmark, or border) immediately on click.

## Acceptance Criteria
1. Single click on a theme option applies it immediately.
2. Active theme is visually highlighted.
3. No double-click required.

## Test Cases
1. Open settings, click a theme — theme applies on first click.
2. Click a different theme — switches immediately.
3. Close and reopen settings — selected theme still indicated.

## Risk
Low. Likely a one-line event handler fix.
