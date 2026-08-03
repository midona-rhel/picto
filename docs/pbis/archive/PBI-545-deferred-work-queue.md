# PBI-545: Persistent deferred work queue for thumbnails and colors

## Priority
P1

## Problem
Deferred thumbnail/color/phash processing currently runs inline or not at all for subscription-imported collection members. When the app closes before deferred work completes, files are left without thumbnails or dominant colors permanently. Need a persistent DB-backed job queue that workers consume.

## Implementation
1. New `deferred_work` table: `(work_id, hash, work_type, status, created_at)`
2. Work types: `thumbnail`, `dominant_colors`, `phash`
3. Import pipeline inserts jobs when skipping work (e.g. `skip_thumbnail: true`)
4. Background worker drains the queue, processing N items per tick
5. Worker respects app shutdown — incomplete jobs stay in queue for next launch
