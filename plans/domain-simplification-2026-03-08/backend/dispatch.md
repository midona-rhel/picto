# Backend Dispatch

## What This Should Own

1. command-name to typed-handler routing
2. argument decoding
3. response encoding

## What This Should Not Own

1. domain policies
2. mutation construction
3. stateful orchestration

## Why It Is Too Complicated

1. `core/src/dispatch` is about 15 files and about 5.2k lines.
2. It still carries history from string-command dispatch while trying to become a typed system.
3. Special cases like pre-state commands make the boundary less obvious than it should be.

## Simplification Target

1. one boring typed command router
2. a very small set of pre-state runtime commands
3. no domain logic in dispatch files

## Concrete Work

1. Make typed dispatch the default and name the few exceptions explicitly.
2. Remove leftover legacy patterns once all commands are typed.
3. Keep argument decoding and serialization in one place.
4. Push all real behavior into domain modules.

## Delete Or Merge

1. Delete dead legacy dispatch helpers after command migration finishes.
2. Merge tiny special-case dispatch handlers if they only exist for historical reasons.

## Test Target

1. one command decoding contract test
2. one unknown-command test
3. one pre-state command test
