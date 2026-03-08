# Tags System Truth

Date: 2026-03-08
Scope: current local tags, subscription/import ingest, PTR compatibility, and frontend tag UI
Audience: whoever has to simplify tags without preserving the current accidental architecture

## What Tags Are For

Tags are the app's main classification system.

That means the tags domain only needs to solve five real problems:

1. store canonical tags on entities
2. let users add, remove, rename, merge, and search tags
3. support alias and parent semantics
4. ingest external tag-like metadata from imports, subscriptions, and PTR
5. expose resolved tags to the UI in a shape the UI can use directly

Everything else is supporting machinery.

## Hard Truth

The current code treats "tags" as one big bucket for several different jobs:

1. canonical local tags
2. Hydrus-compatible cleaning rules
3. ingest-time coercion of foreign metadata
4. sibling and parent graph resolution
5. bitmap compilation for fast queries
6. PTR overlay resolution
7. four separate frontend tag browsing or picking surfaces

That is why the domain feels incoherent. It is not one domain. It is a reasonable domain plus several adapters and several projections that were allowed to blur together.

## What The Product Actually Needs

The product does not need a grand tag framework.

It needs:

1. a canonical tag table: `(namespace, subtag)`
2. a direct membership table: which entities have which tags
3. an alias table: which tags display as another tag
4. a parent table: which tags imply ancestor tags
5. a compiled query projection: so grid, filters, and summaries are fast
6. an ingest adapter boundary: so external metadata can be mapped once
7. a renderer DTO that already contains `raw_tag`, `display_tag`, `namespace`, `subtag`, `source`, and `read_only`

That is the whole thing.

## Current Backend Model

These are the actual local tag primitives today:

1. `tag(namespace, subtag, file_count)`
2. `entity_tag_raw(entity_id, tag_id, source)`
3. `tag_sibling(from_tag_id, to_tag_id, source)`
4. `tag_parent(child_tag_id, parent_tag_id, source)`
5. `tag_ancestor(tag_id, ancestor_id, depth)`
6. `tag_display(tag_id, display_ns, display_st)`
7. `entity_tag_implied(entity_id, tag_id)`

That model is more sensible than the surrounding code makes it look.

The core meaning is:

1. `tag` is canonical storage
2. `entity_tag_raw` is direct assignment
3. `tag_sibling` is alias or display indirection
4. `tag_parent` is implication
5. `tag_ancestor` is compiled transitive parent closure
6. `tag_display` is compiled display resolution for sibling targets
7. `entity_tag_implied` is compiled inherited membership from parents

This is already enough to power the feature set.

## Current Runtime Flow

### 1. Local user tagging

The local path is:

1. typed command in `core/src/dispatch/typed/tags.rs`
2. `TagController`
3. `tags::normalize::parse_tag`
4. `SqliteDatabase::tag_entity` or `untag_entity`
5. compiler events:
   - `FileTagsChanged`
   - `TagChanged`
6. tag graph and bitmap rebuild
7. metadata and grid consumers read compiled results

This is the correct shape in principle, but the controller layer is thinner than it should be and more inconsistent than it should be.

### 2. Import and subscription ingest

The ingest path is different:

1. raw external strings arrive from manual import, subscriptions, or other adapters
2. backend uses `parse_tag_ingest`
3. unknown namespaces are coerced to unnamespaced literal tags
4. the resulting canonical tags are stored like any other local tags

That distinction is valid.

What is not valid is allowing ingest rules to keep leaking into normal renderer behavior long after the data is already stored.

### 3. Parent and sibling compilation

The compiler rebuilds three projections:

1. `tag_ancestor` from `tag_parent`
2. `tag_display` from `tag_sibling`
3. `entity_tag_implied` from `entity_tag_raw + tag_ancestor`

Then it rebuilds:

1. `BitmapKey::ImpliedTag(tag_id)`
2. `BitmapKey::EffectiveTag(tag_id)` as direct OR implied
3. `BitmapKey::Tagged` as union of all effective tagging

This is the right basic strategy.

The code is complicated because the model is spread across `tags`, `sqlite`, `metadata`, `selection`, `grid`, and runtime invalidation concerns, not because the underlying math is hard.

### 4. Metadata read path

The renderer does not really consume raw local storage. It consumes resolved metadata.

The current metadata path:

1. local tags read from `get_entity_tags`
2. local tags mapped to `ResolvedTagInfo`
3. PTR overlay tags loaded separately
4. both combined in `MetadataController`
5. sibling-resolved display tag becomes the public visible tag

That is the correct place to combine sources.

The bad part is that the frontend still reparses some of these tags because the DTO boundary was not treated seriously enough.

## Current PTR Model

PTR is not the local tags system. PTR is a second source of tag facts with its own storage and compatibility requirements.

The current PTR tag path has its own parallel model:

1. `ptr_tag`
2. `ptr_file_tag`
3. `ptr_tag_sibling`
4. `ptr_tag_parent`
5. `ptr_tag_display`
6. `ptr_overlay`

That duplication is not necessarily bad. PTR is a read model and sync system with different operational needs.

The mistake would be trying to make local tags and PTR tags share one giant live implementation.

The correct line is:

1. local tags own editable canonical library state
2. PTR owns synced external repository state
3. metadata composition merges them at read time
4. compatibility logic stays in PTR and in shared low-level normalization only where it is truly required

## Hydrus Compatibility: What Actually Matters

Hydrus compatibility is a real requirement, but only in specific places.

It matters in:

1. tag cleaning and normalization rules in `core/src/tags/normalize.rs`
2. PTR protocol parsing and update processing
3. possibly some import coercion behavior for Hydrus-like external data

It does not justify:

1. renderer-side reparsing of already-resolved tags
2. site-specific subscription metadata rules living as generic tag semantics
3. maintenance operations firing because a tag screen opened

The rule should be:

Hydrus compatibility belongs at normalization and adapter boundaries, not all over the application surface.

## The Real Conceptual Model

If this domain were explained to a new contributor honestly, it would be this:

1. A canonical tag is `(namespace, subtag)`.
2. A direct tag means the entity was explicitly assigned that tag.
3. A sibling means one canonical tag should display as another canonical tag.
4. A parent means one canonical tag implies another canonical tag.
5. An effective tag means direct plus inherited tags after parent expansion.
6. A display tag means the visible tag after sibling resolution.
7. A merge means destructive canonicalization of one stored tag into another.
8. External metadata mapping is a one-time ingest concern, not an always-on semantic rule.

That needs to be written into the code structure, not merely implied by scattered functions.

## What Is Internally Inconsistent Today

### 1. Ingest policy is treated like global tag semantics

`parse_tag_ingest` is reasonable on import and subscription boundaries.

It was not reasonable for the renderer to re-apply namespace policy to general tag strings. That leak has already started getting removed, but the bigger issue remains: the codebase still acts like ingest coercion defines the whole tag domain.

It does not.

### 2. Subscription tag extraction is living in the wrong conceptual place

`core/src/subscriptions/gallery_dl_runner.rs` contains a large amount of site-specific tag extraction and tag-category mapping.

That may be unavoidable as an adapter.

What is not acceptable is letting that code define what tags mean for the rest of the application. Danbooru, Pixiv, and E621 parsing are adapter rules. They are not the local tag model.

### 3. The backend call path is split without a clear reason

The current local tag call path crosses:

1. typed dispatch
2. `TagController`
3. `SqliteDatabase`
4. compiler events

That would be fine if each layer had a clear ownership sentence.

Right now it does not:

1. `TagController` mostly normalizes and remaps DTOs
2. `SqliteDatabase` owns both persistence and a lot of behavioral policy
3. command handlers sometimes feel like thin pass-throughs, sometimes not

This is not a sound service boundary. It is historical sediment.

### 4. Rename, merge, alias, and parent are too implicit

These are four different actions:

1. rename: same tag id, new canonical string
2. merge: two canonical tags collapse into one
3. sibling: display alias relation, not destructive merge
4. parent: inheritance relation, not aliasing

The code supports all four, but the distinction is not made obvious enough in the API or UI.

That is how users and contributors end up thinking the logic "doesn't make sense."

### 5. Frontend surfaces duplicate each other badly

Current surfaces:

1. `TagManager`
2. `TagSelectPanel`
3. `TagPickerPortal`
4. `TagPickerMenu`
5. inspector tag display and mutation flows

These repeat fetching, grouping, parsing, searching, and selection behavior in slightly different forms.

This is not flexibility. It is duplication.

### 6. Some maintenance behavior is in user-facing flows

The worst example was `normalize_ingested_namespaces` being triggered from normal `TagManager` mount behavior.

That is architecture failure.

Opening a screen must never silently rewrite the library's tag model.

## What The Simplified Domain Should Look Like

## Backend Target

One local tags service should own:

1. parse local tag input
2. parse external ingest input
3. add and remove direct tags
4. rename and merge tags
5. add and remove sibling relations
6. add and remove parent relations
7. search and namespace summary
8. emit one coherent set of compiler or mutation invalidations

That service can still call SQLite functions internally. The point is not to eliminate modules. The point is to stop pretending controller and database layers are separate domains when they are not.

## Frontend Target

The frontend should have:

1. one tag DTO model coming from the backend
2. one tag browser or manager view model
3. one reusable picker list model
4. thin presentation shells for manager, picker, and inspector

The frontend should not own:

1. ingest namespace policy
2. canonicalization policy
3. relation semantics
4. maintenance workflows

## Compatibility Target

The compatibility story should be explicit:

1. Hydrus-compatible cleaning stays in one low-level backend module
2. PTR keeps its own storage and sync model
3. subscriptions keep site-specific metadata extraction in adapters
4. the local editable tag domain remains small and product-focused

That is how you stay compatible without turning the whole app into Hydrus cosplay.

## Rewrite Boundaries

If this domain is going to be rewritten or heavily simplified, the boundaries should be:

### Keep

1. canonical `(namespace, subtag)` storage
2. direct tag membership
3. sibling and parent relations
4. compiled effective-tag projection
5. Hydrus cleaning logic in one backend module
6. PTR as a parallel read model

### Collapse

1. thin controller logic into one obvious service layer
2. repeated frontend tag parsing into backend DTO consumption
3. duplicate picker implementations into one shared tag list model
4. repeated search and grouping logic across tag surfaces

### Delete

1. UI-triggered maintenance rewrites
2. frontend attempts to re-decide ingest namespace validity
3. comments that narrate old migration intent instead of current behavior
4. tiny tests that only pin internal wiring while ignoring the real workflows

## What A Sane Test Strategy Looks Like

Stop testing tags like a pile of helpers and start testing the actual workflows.

The domain needs a small number of integration-heavy tests:

1. create file -> add tag -> remove tag -> file metadata and search both update
2. rename tag without merge -> metadata and search reflect new canonical tag
3. rename tag into existing tag -> merge behavior is explicit and verified
4. add sibling -> display changes, canonical storage does not
5. add parent -> effective tags and "untagged" logic update correctly
6. import external tags with unknown namespace -> coercion happens once on ingest
7. combine local and PTR tags in metadata -> read-only and display behavior are correct

That is more valuable than fifteen small tests proving a picker remembered a local state toggle.

## Ordered Rewrite Plan

### Phase 1: Freeze the model

1. Treat this document as the source of truth.
2. Stop adding new tag wrappers.
3. Remove stale comments that explain migration history instead of runtime behavior.

### Phase 2: Clarify backend ownership

1. Create one obvious local tags service surface.
2. Move policy out of scattered command handlers where appropriate.
3. Make rename, merge, sibling, and parent distinct in the API shape and naming.
4. Keep `parse_tag` and `parse_tag_ingest` backend-only.

### Phase 3: Isolate adapters

1. Move subscription site-specific mapping mentally and structurally under ingest adapters.
2. Keep PTR relation resolution under PTR overlay or PTR tag storage.
3. Stop calling adapter logic "tag semantics."

### Phase 4: Fix renderer ownership

1. Normalize DTOs at the backend boundary.
2. Remove remaining renderer reparsing of canonical or resolved tag data.
3. Replace duplicated picker surfaces with one shared tag list model.

### Phase 5: Replace test sprawl

1. Delete low-value micro-tests around internal component behavior.
2. Add end-to-end domain workflow tests around tagging, relation editing, and metadata resolution.

## Blunt Recommendation

If the team is serious about simplification, tags should be treated as a rewrite-sized cleanup target, not a "small refactor."

Not because the underlying tag model is unsalvageable.

It is actually mostly fine.

The problem is that too many unrelated concerns have been allowed to accumulate around it:

1. adapter rules
2. compatibility rules
3. projections
4. UI duplication
5. stale comments and migration layers

The right move is not to throw away every useful piece.

The right move is:

1. keep the canonical storage model
2. keep the compiled projection model
3. keep Hydrus compatibility where it genuinely matters
4. rewrite the domain boundaries and frontend surfaces around those truths

That is how this gets smaller and more coherent instead of just being rearranged.
