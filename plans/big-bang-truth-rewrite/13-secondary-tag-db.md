# Secondary Tag DB

## Purpose

Document the dormant hydrus-derived secondary tag database as an internal subsystem only.

## Current Truth

- PTR still leaks into settings, tags UI, runtime UI, API surface, and generated command types.
- That makes dormant infrastructure look like product architecture.

## Target Truth

- The subsystem is named `secondary_tag_db` in docs and internal code.
- It is hidden from product UI.
- It is hidden from live frontend runtime surfaces.
- It remains sealed behind backend-only boundaries until it is real product functionality.

## Rename Map

- visible `PTR` -> removed
- internal `ptr` module may remain temporarily but is documented as target `secondary_tag_db`

## Delete List

- Delete `PtrPanel` from settings navigation.
- Delete PTR mode in tags UI.
- Delete PTR job cards from sidebar runtime UI.
- Delete PTR public frontend API surface after internal boundary replacement.

## DTOs and Commands Involved

- current `api.ptr.*` surface is targeted for removal from renderer product code
- backend-only sync/bootstrap/status boundary may remain temporarily

## Workflows

- No product workflow references this subsystem.
- If enabled internally, it populates a secondary tag database without exposing direct UI affordances.

## Acceptance Criteria

- End users cannot discover PTR from normal product UI.
- Main app flows do not branch on PTR.
- Remaining secondary-tag-db code is clearly internal and sealed.
