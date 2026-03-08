# Backend Lifecycle

Current footprint: about 2 files and about 51 lines

## What This Should Own

1. file lifecycle transitions
2. status changes and deletion semantics if this remains a distinct domain

## What This Should Not Own

1. become a fake domain with no real boundary

## Why It Is Too Complicated

1. This is the opposite problem: it may be too small to deserve a standalone domain.
2. If lifecycle is just file state transitions, it probably belongs under files or import.

## Simplification Target

1. either make lifecycle a real file-state submodule
2. or delete it as a top-level concept

## Concrete Work

1. Decide if lifecycle owns a distinct policy.
2. If not, fold it into files or import and stop pretending it is separate.

## Delete Or Merge

1. Merge lifecycle into the file domain unless a clear boundary appears.

## Test Target

1. file status transition workflow
2. delete or trash workflow
