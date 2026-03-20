# PBI-525: AI auto-tagging (WD14 / E621 tagger models)

## Priority
P1

## Problem
Users manually tag imported images, which is tedious for large libraries. Booru-style tagger models (SmilingWolf's WDv3, E621) can automatically predict tags with confidence scores. The existing tag namespaces (`general`, `character`, `copyright`, `artist`, `rating`, `species`) already match WD14/E621 output categories perfectly, but the codebase has no ML inference capability today.

Users need:
- On-demand tagging from the inspector (predict → review → accept/reject)
- Optional auto-tagging on import/download
- Per-category confidence thresholds (character/artist need high thresholds to avoid false positives on proper nouns; general tags can be lower)
- GPU-accelerated inference on all platforms with CPU fallback

## Scope

### Rust Core (`core/src/`)
- New `ai_tagger/` module: model registry, ONNX inference, label CSV parsing, model download
- `settings/store.rs`: AI tagging settings (enabled, model, per-category thresholds, auto-on-import)
- `state.rs`: `TaggerSession` field on `AppState`
- `dispatch/typed/ai_tagger.rs`: New commands (`ai_tagger_status`, `ai_tagger_download_model`, `ai_tag_predict`, `ai_tag_apply`)
- `dispatch/mod.rs`: Command routing for new commands
- `runtime_contract/task.rs`: `ModelDownload` variant for `TaskKind`
- `import/service.rs`: Optional post-import AI tagging hook

### Frontend (`src/`)
- `features/settings/components/AiTaggingPanel.tsx`: Settings panel (model selection, thresholds, automation toggle)
- `features/settings/components/Settings.tsx`: Register panel in sidebar
- `features/inspector/components/InspectorPanel.tsx`: Auto-Tag button in all three views
- `features/inspector/components/AiTagReviewModal.tsx`: Prediction review modal
- `features/inspector/hooks/useInspectorMutations.ts`: `onAutoTag` callback with undo
- `platform/api.ts`: `aiTagger` API namespace
- `state/settingsStore.ts`: AI settings fields

### Dependencies
- `ort = { version = "2", features = ["load-dynamic"] }` added to `core/Cargo.toml`
- ONNX Runtime shared library bundled in Electron package resources (~20MB CPU-only)

## Implementation

### Phase 1: Rust core — ONNX inference engine

1. **Add `ort` dependency** to `core/Cargo.toml` with `load-dynamic` feature (loads ONNX Runtime shared libs at runtime — GPU backends are optional, app works without them).

2. **Create `core/src/ai_tagger/` module** with submodules:
   - `mod.rs` — module root, `TaggerModel` enum
   - `models.rs` — model registry with known configs (HuggingFace URLs, input sizes, category mappings) for WD14 ViT/SwinV2/ConvNext and E621 models
   - `labels.rs` — CSV label parsing; WD14/E621 `selected_tags.csv` format maps category integers (0→general, 1→artist, 3→copyright, 4→character, 5→species, 9→rating) to Picto namespaces
   - `inference.rs` — `TaggerSession` struct wrapping `ort::Session`; image preprocessing (resize to model input size, RGB f32 `[1,3,H,W]` tensor, normalize to [0,1]); sigmoid on logits; threshold filtering per category; returns `Vec<TagPrediction>` with `(namespace, tag_name, confidence)`
   - `download.rs` — model download via `reqwest` with `RuntimeTask` progress tracking; writes to `{appData}/picto/models/{slug}/model.onnx.part` then renames on completion; also downloads `selected_tags.csv`

3. **GPU backend selection** in `inference.rs`: try providers in platform order (macOS: CoreML→CPU; Windows: CUDA→DirectML→CPU; Linux: CUDA→CPU). `ort` falls back automatically if a GPU provider fails to init.

4. **Settings extension** in `core/src/settings/store.rs` — add to `AppSettings`:
   - `ai_tagger_enabled: bool` (default false)
   - `ai_tagger_auto_on_import: bool` (default false)
   - `ai_tagger_model: String` (default "wd14-swinv2-v3")
   - `ai_threshold_general: f32` (default 0.35)
   - `ai_threshold_character: f32` (default 0.85)
   - `ai_threshold_copyright: f32` (default 0.85)
   - `ai_threshold_artist: f32` (default 0.85)
   - `ai_threshold_species: f32` (default 0.35)
   - `ai_threshold_rating: f32` (default 0.50)

5. **AppState extension** — add `ai_tagger: Arc<tokio::sync::Mutex<Option<TaggerSession>>>` to `AppState` in `state.rs`. Lazy init on first use, released on library close.

6. **Dispatch commands** in new `core/src/dispatch/typed/ai_tagger.rs`:
   - `ai_tagger_status` — returns enabled state, model download status, detected GPU backend, available model list
   - `ai_tagger_download_model` — triggers async download, returns immediately, progress via RuntimeTask
   - `ai_tag_predict` — run inference on hash(es), return predictions WITHOUT applying (for review modal)
   - `ai_tag_apply` — apply accepted tags with `source = "ai"` via existing `tag_entity()` (which already accepts a source parameter)

7. **RuntimeTask for downloads** — add `ModelDownload` variant to `TaskKind` in `runtime_contract/task.rs`.

8. **Import pipeline hook** — in `import/service.rs`, after successful import: if `ai_tagger_auto_on_import` enabled and model downloaded, run inference on imported image bytes and apply via `add_tags_batch_by_entity_ids()` with `source = "ai"`.

### Phase 2: Frontend — settings panel

9. **AI Tagging settings panel** (`AiTaggingPanel.tsx`) using existing `SettingsBlock`/`SettingsRow` components:
   - Model block: enable toggle, model dropdown (WD14 ViT/SwinV2/ConvNext/E621), GPU backend badge, download button or "Ready" status
   - Thresholds block: per-category sliders (0–100%) for general, character, copyright, artist, species (E621 only), rating
   - Automation block: auto-tag on import toggle

10. **Register in settings sidebar** in `Settings.tsx` after the Duplicates entry.

11. **Frontend settings store** — add AI fields to `AppSettings` interface in `settingsStore.ts`.

12. **API surface** — add `aiTagger` namespace to `api.ts` with `status()`, `downloadModel()`, `predict()`, `apply()`.

### Phase 3: Frontend — inspector auto-tag button & review modal

13. **Inspector Auto-Tag button** — add button next to Export in all three inspector views (virtual selection at line 579, single image at line 619, multi-selection at line 649). Enabled when AI tagger enabled + model downloaded + images selected.

14. **Tag prediction review modal** (`AiTagReviewModal.tsx`):
    - On open: calls `api.aiTagger.predict(hashes)` with loading spinner
    - Body: tags grouped by namespace, each group collapsible, sorted by confidence descending
    - Each tag row: checkbox (default checked) + tag name + confidence percentage
    - Group headers: select all/deselect all toggle + count badge
    - Footer: "Apply N tags" primary button + Cancel
    - On Apply: calls `api.aiTagger.apply()`, refreshes metadata, registers undo action

15. **Mutations integration** — add `onAutoTag` callback in `useInspectorMutations.ts` with undo support via existing `registerUndoAction` pattern.

### Phase 4: ONNX Runtime bundling

16. **Bundle ONNX Runtime CPU library** in Electron package resources (~20MB per platform). At startup, point `ort::init_from()` to bundled path. GPU-enabled variants (200–500MB) can be downloaded on demand in future work.

## Acceptance Criteria
1. Settings panel renders with model selection, per-category threshold sliders, and auto-import toggle
2. Model download starts on user action, shows progress in task overlay, completes to `{appData}/picto/models/`
3. Inspector "Auto-Tag" button triggers inference and opens prediction review modal
4. Modal displays tags grouped by namespace with confidence scores; user can accept/reject individual tags
5. Accepted tags stored in `entity_tag_raw` with `source = "ai"` and display correctly in inspector tag list
6. Auto-tag on import applies AI tags to each imported file when enabled, visible immediately in grid/inspector
7. GPU acceleration uses CoreML on macOS, CUDA on Linux/Windows, DirectML as Windows fallback, CPU as universal fallback
8. Tag compiler automatically indexes AI-sourced tags (existing `ReadModelEvent::FileTagsChanged` propagation handles this — no special handling needed)
9. Undo works for AI tag application (removes applied tags on undo)
10. `cargo check` and `npx tsc --noEmit` pass

## Test Cases
1. **Unit (Rust):** Label CSV parsing produces correct namespace mapping for WD14 and E621 formats (category 0→general, 4→character, etc.)
2. **Unit (Rust):** Image preprocessing produces correct tensor shape `[1, 3, 448, 448]` for WD14, `[1, 3, 448, 448]` for E621
3. **Unit (Rust):** Sigmoid + threshold filtering correctly selects tags above threshold and rejects those below
4. **Unit (Rust):** `tag_entity()` with `source = "ai"` inserts correctly and coexists with existing `source = "local"` tags on the same entity
5. **Integration (Rust):** Full pipeline: load model, predict on test image, verify tag output matches expected labels
6. **Integration (Rust):** Model download to temp dir, verify file integrity (size / checksum)
7. **Frontend:** Settings panel renders; toggle enables/disables; threshold changes persist across navigation
8. **Frontend:** Auto-Tag button disabled when no selection or model not downloaded
9. **Frontend:** Prediction modal displays grouped tags; accept/reject toggles work; Apply triggers API call and tags appear in inspector
10. **E2E:** Import file with auto-tag enabled → file appears in grid with AI-predicted tags visible in inspector

## Risk
Medium. Key risks and mitigations:
- **ONNX Runtime library size** (~20MB CPU) increases app package — acceptable for the feature value
- **Model file size** (100–400MB) requires download UX — handled via RuntimeTask progress bar with resume support
- **Inference latency** on CPU (~100–300ms per image) could slow batch imports — GPU reduces to ~5–20ms; auto-tag on import can be disabled by user
- **`ort` crate cross-platform compatibility** — use `load-dynamic` feature, pin version, bundle known-good ONNX Runtime release, test on all three platforms
- **Tag quality** varies by model and threshold — per-category thresholds let users tune precision/recall; review modal lets users filter before applying; `"ai"` source tag allows bulk removal later if needed
