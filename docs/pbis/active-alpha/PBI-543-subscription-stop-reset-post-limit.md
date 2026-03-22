# PBI-543: Subscription stop, reset, and post limit issues

## Priority
P1

## Issues

### Stop not working on Windows
- Pressing stop during a subscription run doesn't halt gallery-dl on Windows. The process continues downloading.

### Reset doesn't clear file/post counts
- Resetting a subscription query doesn't zero out the accumulated `files_found` and `posts_found` counters.

### 100 post cap per run
- Gallery-dl appears to cap at 100 posts per run regardless of the configured post limit. May be related to `--range` arg construction or coomer/kemono site behavior.

### Stops on finding duplicate
- Subscription appears to stop early when encountering a previously-downloaded post, even during initial pagination where it should continue.

### Names not imported properly for Coomer/Kemono
- Post titles from coomer.st and kemono.cr are not being set as the file/collection name during import. May be a metadata parsing issue in the gallery-dl sidecar.
