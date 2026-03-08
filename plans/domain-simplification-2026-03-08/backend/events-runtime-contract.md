# Backend Events And Runtime Contract

## What This Should Own

1. mutation receipts
2. task upsert and removal events
3. event-name constants
4. structured runtime payload types

## What This Should Not Own

1. duplicate legacy event systems
2. per-domain UI refresh policy outside mutation facts

## Why It Is Too Complicated

1. `core/src/events.rs` still exposes legacy compatibility events beside the newer runtime task model.
2. The frontend contract and backend event names have already drifted.
3. The project now has a "right" event system and a "still around" event system at the same time.

## Simplification Target

1. one authoritative runtime event model
2. mutation facts for invalidation
3. task events for long-running work
4. only a tiny set of non-task system events

## Concrete Work

1. Decide whether legacy compatibility events live or die.
2. If they live, type them and document them as transitional only.
3. If they die, move remaining consumers and delete them.
4. Keep runtime contract generation aligned with frontend types and guards.

## Delete Or Merge

1. Delete compatibility events once the renderer no longer consumes them.
2. Merge event naming and contract definitions into one obvious place if possible.

## Test Target

1. one schema parity check that actually reflects current architecture
2. mutation receipt generation tests
3. task lifecycle event tests
