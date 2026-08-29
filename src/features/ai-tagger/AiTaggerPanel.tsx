/**
 * AI review portal.
 *
 * Predictions are reviewed per root and remain read-only until applied.
 * Collection image predictions are unioned before one root-level assignment.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import {
  IconCheck,
  IconBookmark,
  IconChevronLeft,
  IconChevronRight,
  IconLayoutSidebar,
  IconSearch,
} from '@tabler/icons-react';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { ProgressBar } from '../../shared/ui/ProgressBar/ProgressBar';
import { ThumbnailImage } from '../../shared/ui/ThumbnailImage/ThumbnailImage';
import { aiTaggerPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import {
  aiTagApply,
  aiTagPredict,
  aiTaggerStatus,
  aiTaggerUnload,
  type AiModelStatus,
  type AiPredictionTarget,
  type AiTagPrediction,
  type RootPrediction,
} from '../../platform/aiTaggerApi';
import { viewerController } from '../../controllers/viewerController';
import type { CanonicalEntityDetails, MediaRecord } from '../../shared/types/canonical';
import { mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';
import inspectorStyles from '../inspector/Inspector.module.css';
import { tagGroupColor } from '../tags/tagGroupPresentation';
import styles from './AiTaggerPanel.module.css';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { TitlebarRangeSlider } from '../../shared/ui/TitlebarControls';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { labToHex } from '../../shared/lib/labColor';
import { getSettings, patchSettings } from '../../platform/settingsApi';

type ViewMode = 'suggested' | 'below';

const MIN_REVIEW_CONFIDENCE = 5;
const MAX_REVIEW_CONFIDENCE = 95;
const DEFAULT_REVIEW_CONFIDENCE = 35;
const PREDICTION_BATCH_SIZE = 4;

interface ReviewTag {
  key: string;
  namespace: string;
  subtag: string;
  confidence: number;
  models: Array<{ slug: string; confidence: number }>;
}

interface RunProgress {
  done: number;
  total: number;
  currentItemId: number | null;
}

interface ReviewRoot {
  rootItemId: number;
  label: string;
  imageMedia: MediaRecord[];
  previewMedia: MediaRecord;
}

function predictionTags(prediction: RootPrediction | undefined, runModels: Set<string>): ReviewTag[] {
  if (!prediction) return [];
  const byKey = new Map<string, ReviewTag>();
  for (const tag of prediction.predictions as AiTagPrediction[]) {
    if (!runModels.has(tag.model)) continue;
    const key = tag.namespace ? `${tag.namespace}:${tag.tag}` : tag.tag;
    const current = byKey.get(key) ?? {
      key,
      namespace: tag.namespace,
      subtag: tag.tag,
      confidence: 0,
      models: [],
    };
    current.confidence = Math.max(current.confidence, tag.confidence);
    const model = current.models.find((entry) => entry.slug === tag.model);
    if (model) model.confidence = Math.max(model.confidence, tag.confidence);
    else current.models.push({ slug: tag.model, confidence: tag.confidence });
    byKey.set(key, current);
  }
  return [...byKey.values()].sort((a, b) => b.confidence - a.confidence);
}

function mergePredictionResults(previous: RootPrediction[], incoming: RootPrediction[]): RootPrediction[] {
  const byItem = new Map(previous.map((entry) => [entry.rootId, entry]));
  for (const next of incoming) {
    const current = byItem.get(next.rootId);
    const predictions = new Map<string, AiTagPrediction>();
    for (const tag of [...(current?.predictions ?? []), ...next.predictions]) {
      const key = `${tag.model}\u0000${tag.namespace}\u0000${tag.tag}`;
      const existing = predictions.get(key);
      if (!existing || tag.confidence > existing.confidence) predictions.set(key, tag);
    }
    byItem.set(next.rootId, {
      rootId: next.rootId,
      predictions: [...predictions.values()],
      error: current?.error ?? next.error,
    });
  }
  return [...byItem.values()];
}

function reviewRoot(details: CanonicalEntityDetails): ReviewRoot | null {
  const imageMedia = details.media.filter((media) => media.facts.mime.startsWith('image/'));
  if (imageMedia.length === 0) return null;
  const previewMedia = imageMedia.find((media) => media.media_id === details.root.cover_media_id)
    ?? imageMedia[0];
  return {
    rootItemId: details.root.root_id,
    label: details.root.name || previewMedia.media_name || `Item ${details.root.root_id}`,
    imageMedia,
    previewMedia,
  };
}

function overrideKey(itemId: number, tag: string): string {
  return `${itemId}\u0000${tag}`;
}

export function AiTaggerPanel() {
  const portalState = useAtomValue(aiTaggerPortalAtom);
  const setPortalState = useSetAtom(aiTaggerPortalAtom);
  const selectionTarget = useAtomValue(selectionTargetAtom);
  const target = portalState.target ?? selectionTarget;
  const open = portalState.open;
  const anchorPosition = portalState.anchor ?? null;

  const [pinned, setPinned] = useState(false);
  const [showSidebar, setShowSidebar] = useState(true);
  const [query, setQuery] = useState('');
  const [models, setModels] = useState<AiModelStatus[]>([]);
  const [runModels, setRunModels] = useState<Set<string>>(new Set());
  const [predictions, setPredictions] = useState<RootPrediction[]>([]);
  const [reviewRoots, setReviewRoots] = useState<ReviewRoot[]>([]);
  const [activeItemId, setActiveItemId] = useState<number | null>(null);
  const [progress, setProgress] = useState<RunProgress>({ done: 0, total: 0, currentItemId: null });
  const [backend, setBackend] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confidence, setConfidence] = useState(DEFAULT_REVIEW_CONFIDENCE);
  const [confidenceDraft, setConfidenceDraft] = useState(DEFAULT_REVIEW_CONFIDENCE);
  const [viewMode, setViewMode] = useState<ViewMode>('suggested');
  const [overrides, setOverrides] = useState<Map<string, boolean>>(new Map());
  const searchRef = useRef<HTMLInputElement>(null);
  const runGenerationRef = useRef(0);
  const runningRef = useRef(false);
  const settingsWriteRef = useRef<Promise<unknown>>(Promise.resolve());

  const itemIds = useMemo(() => (target?.kind === 'explicit' ? target.root_ids : []), [target]);
  const itemFingerprint = itemIds.join('\n');

  const closePortal = useCallback(() => {
    runGenerationRef.current += 1;
    runningRef.current = false;
    setRunning(false);
    void aiTaggerUnload();
    setPortalState({ open: false });
  }, [setPortalState]);

  const runPredict = useCallback(async (slugs: Set<string>, roots: ReviewRoot[], reset = false) => {
    if (runningRef.current || slugs.size === 0 || roots.length === 0) return;
    const generation = ++runGenerationRef.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    const orderedSlugs = [...slugs];
    if (reset) setPredictions([]);

    try {
      const targets: AiPredictionTarget[] = roots.flatMap((root) => root.imageMedia.map((media) => ({
        rootId: root.rootItemId,
        mediaItemId: media.media_id,
      })));
      const total = orderedSlugs.length * targets.length;
      setProgress({ done: 0, total, currentItemId: targets[0]?.rootId ?? null });
      let failedAnalyses = 0;
      let firstFailure: string | null = null;
      let done = 0;
      for (const slug of orderedSlugs) {
        if (generation !== runGenerationRef.current) return;
        for (let offset = 0; offset < targets.length; offset += PREDICTION_BATCH_SIZE) {
          const batch = targets.slice(offset, offset + PREDICTION_BATCH_SIZE);
          setProgress({ done, total, currentItemId: batch[0]?.rootId ?? null });
          const output = await aiTagPredict(batch, [slug]);
          if (generation !== runGenerationRef.current) return;
          setPredictions((previous) => mergePredictionResults(previous, output.predictions));
          setActiveItemId((current) => current ?? batch[0]?.rootId ?? null);
          const errors = new Map(output.predictions.flatMap((prediction) => (
            prediction.error ? [[prediction.rootId, prediction.error] as const] : []
          )));
          failedAnalyses += batch.filter((target) => errors.has(target.rootId)).length;
          firstFailure ??= errors.values().next().value ?? null;
          done += batch.length;
          setProgress({ done, total, currentItemId: null });
        }
      }
      if (failedAnalyses > 0) {
        setError(`${failedAnalyses} of ${total} model/media analyses failed. ${firstFailure}`);
      }
      try {
        const status = await aiTaggerStatus();
        if (generation === runGenerationRef.current) setBackend(status.cachedBackend ?? 'CPU');
      } catch {
        if (generation === runGenerationRef.current) setBackend((current) => current ?? 'CPU');
      }
    } catch (reason) {
      if (generation === runGenerationRef.current) setError(String(reason));
    } finally {
      if (generation === runGenerationRef.current) {
        runningRef.current = false;
        setRunning(false);
        setProgress((current) => ({ ...current, currentItemId: null }));
      }
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    const generation = ++runGenerationRef.current;
    runningRef.current = false;
    setQuery('');
    setPredictions([]);
    setReviewRoots([]);
    setActiveItemId(null);
    setOverrides(new Map());
    setViewMode('suggested');
    setError(null);
    setBackend(null);
    setRunning(false);
    setProgress({ done: 0, total: 0, currentItemId: null });
    setConfidence(DEFAULT_REVIEW_CONFIDENCE);
    setConfidenceDraft(DEFAULT_REVIEW_CONFIDENCE);

    void Promise.all([
      aiTaggerStatus(),
      Promise.allSettled(itemIds.map((itemId) => viewerController.getItemDetails(itemId))),
      getSettings(),
    ]).then(([status, detailResults, settings]) => {
      if (generation !== runGenerationRef.current) return;
      const roots = detailResults.flatMap((result) => {
        if (result.status !== 'fulfilled') return [];
        const root = reviewRoot(result.value);
        return root ? [root] : [];
      });
      const detailFailures = detailResults.filter((result) => result.status === 'rejected');
      const unsupported = detailResults.filter((result) => (
        result.status === 'fulfilled' && reviewRoot(result.value) == null
      ));
      setModels(status.models);
      const initialConfidence = Math.min(
        MAX_REVIEW_CONFIDENCE,
        Math.max(MIN_REVIEW_CONFIDENCE, Math.round((status.thresholds.general ?? DEFAULT_REVIEW_CONFIDENCE / 100) * 100)),
      );
      setConfidence(initialConfidence);
      setConfidenceDraft(initialConfidence);
      setBackend(status.cachedBackend);
      setReviewRoots(roots);
      setActiveItemId(roots[0]?.rootItemId ?? null);
      if (detailFailures.length > 0) {
        setError(`${detailFailures.length} selected items could not be prepared for AI review.`);
      } else if (unsupported.length > 0) {
        setError(`${unsupported.length} selected items contain no images.`);
      }
      const downloaded = new Set(status.models.filter((model) => model.downloaded).map((model) => model.slug));
      const remembered = settings.aiTaggerManualModelSlugs;
      const previous = (remembered ?? status.configuredModelSlugs).filter((slug) => downloaded.has(slug));
      const fallback = status.models.find((model) => model.downloaded && model.recommended)
        ?? status.models.find((model) => model.downloaded);
      const initial = remembered === null && previous.length === 0 && fallback
        ? [fallback.slug]
        : previous;
      setRunModels(new Set(initial));
    }).catch((reason) => {
      if (generation === runGenerationRef.current) setError(String(reason));
    });

    const focusTimer = setTimeout(() => searchRef.current?.focus(), 50);
    return () => {
      clearTimeout(focusTimer);
      runGenerationRef.current += 1;
      runningRef.current = false;
      void aiTaggerUnload();
    };
  }, [open, itemFingerprint]);

  const reviewItemIds = useMemo(() => reviewRoots.length > 0
    ? reviewRoots.map((root) => root.rootItemId)
    : predictions.map((prediction) => prediction.rootId), [predictions, reviewRoots]);
  const activeIndex = Math.max(0, reviewItemIds.indexOf(activeItemId ?? reviewItemIds[0]));
  const activeRoot = reviewRoots.find((root) => root.rootItemId === activeItemId) ?? reviewRoots[activeIndex] ?? null;
  const activeMedia = activeRoot?.previewMedia ?? null;
  const activePrediction = predictions.find((prediction) => prediction.rootId === activeItemId);

  const moveActive = useCallback((delta: number) => {
    if (reviewItemIds.length === 0) return;
    const current = Math.max(0, reviewItemIds.indexOf(activeItemId ?? reviewItemIds[0]));
    const next = Math.max(0, Math.min(reviewItemIds.length - 1, current + delta));
    setActiveItemId(reviewItemIds[next]);
  }, [activeItemId, reviewItemIds]);

  useShortcutScope((event) => {
    const previous = getShortcut('view.prevImage');
    const next = getShortcut('view.nextImage');
    if (previous && matchesShortcutDef(event, previous)) {
      moveActive(-1);
      return true;
    }
    if (next && matchesShortcutDef(event, next)) {
      moveActive(1);
      return true;
    }
    return false;
  }, { enabled: open, priority: 100 });

  const toggleModel = useCallback((slug: string) => {
    const next = new Set(runModels);
    if (next.has(slug)) next.delete(slug);
    else next.add(slug);
    setRunModels(next);
    const selected = [...next];
    settingsWriteRef.current = settingsWriteRef.current
      .catch(() => undefined)
      .then(() => patchSettings({ aiTaggerManualModelSlugs: selected }))
      .catch((reason) => setError(`Could not remember model selection. ${String(reason)}`));
  }, [runModels]);

  const confidenceCutoff = confidence / 100;
  const activeTags = useMemo(() => predictionTags(activePrediction, runModels), [activePrediction, runModels]);
  const suggested = useMemo(() => activeTags.filter((tag) => tag.confidence >= confidenceCutoff), [activeTags, confidenceCutoff]);
  const below = useMemo(() => activeTags.filter((tag) => tag.confidence < confidenceCutoff), [activeTags, confidenceCutoff]);
  const isChecked = useCallback((itemId: number, tag: ReviewTag) => (
    overrides.get(overrideKey(itemId, tag.key))
      ?? tag.confidence >= confidenceCutoff
  ), [confidenceCutoff, overrides]);

  const assignments = useMemo(() => predictions.flatMap((prediction) => {
    const tags = predictionTags(prediction, runModels)
      .filter((tag) => isChecked(prediction.rootId, tag))
      .map((tag) => tag.key);
    const root = reviewRoots.find((candidate) => candidate.rootItemId === prediction.rootId);
    return tags.length > 0 && root
      ? [{ root_id: root.rootItemId, tags }]
      : [];
  }), [isChecked, predictions, reviewRoots, runModels]);
  const checkedCount = assignments.reduce((total, assignment) => total + assignment.tags.length, 0);

  const applyChecked = useCallback(async () => {
    if (assignments.length === 0) return;
    setApplying(true);
    setError(null);
    try {
      await aiTagApply(assignments);
      await announceUndoableMutation('items.apply_ai_tags');
      setOverrides(new Map());
      if (!pinned) setPortalState({ open: false });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setApplying(false);
    }
  }, [assignments, pinned, setPortalState]);

  const visibleTags = useMemo(() => {
    const source = viewMode === 'suggested' ? suggested : below;
    const search = query.trim().toLowerCase();
    return search ? source.filter((tag) => tag.key.toLowerCase().includes(search)) : source;
  }, [below, query, suggested, viewMode]);

  const modelLabel = useCallback((slug: string) => models.find((model) => model.slug === slug)?.label ?? slug, [models]);
  const modelCounts = useMemo(() => new Map(models.map((model) => {
    const keys = new Set<string>();
    for (const prediction of predictions) {
      for (const tag of prediction.predictions) {
        if (tag.model === model.slug) keys.add(`${prediction.rootId}\u0000${tag.namespace}:${tag.tag}`);
      }
    }
    return [model.slug, keys.size];
  })), [models, predictions]);

  if (!open) return null;

  const currentNumber = reviewItemIds.length > 0 ? activeIndex + 1 : 0;
  const activeIsRunning = running && progress.currentItemId === activeItemId;
  const commitConfidence = () => setConfidence(confidenceDraft);

  return (
    <OverlayShell
      open={open}
      onClose={closePortal}
      width={showSidebar ? 780 : 590}
      height={540}
      pinned={pinned}
      anchorPosition={anchorPosition}
      onPinnedChange={setPinned}
      header={
        <>
          <div className={shellStyles.searchRow} style={{ flex: 1 }}>
            <IconSearch size={14} className={shellStyles.searchIcon} />
            <input ref={searchRef} className={shellStyles.searchInput} placeholder="Filter suggestions..." value={query} onChange={(event) => setQuery(event.target.value)} />
          </div>
          {running && <span className={styles.runCounter}>Analyzing {Math.min(progress.done + 1, progress.total)} of {progress.total}</span>}
          <KbdTooltip label={showSidebar ? 'Hide sidebar' : 'Show sidebar'}><button className={shellStyles.pinBtn} onClick={() => setShowSidebar((value) => !value)} type="button" aria-label={showSidebar ? 'Hide sidebar' : 'Show sidebar'}>
            <IconLayoutSidebar size={14} />
          </button></KbdTooltip>
        </>
      }
      footer={
        <>
          <div className={styles.confidenceControl}>
            <span>Confidence</span>
            <TitlebarRangeSlider
              aria-label="Run confidence"
              min={MIN_REVIEW_CONFIDENCE}
              max={MAX_REVIEW_CONFIDENCE}
              step={1}
              value={confidenceDraft}
              onValueChange={setConfidenceDraft}
              onPointerUp={commitConfidence}
              onKeyUp={commitConfidence}
              onBlur={commitConfidence}
              className={styles.confidenceSlider}
            />
            <span className={styles.confidenceValue}>{confidenceDraft}%</span>
            <span className={styles.inferenceBackend}>· {backend ? `${backend} inference` : 'Local inference'}</span>
          </div>
          <div className={btnStyles.btnGroup}>
            <span className={shellStyles.kbdHint}><span className={shellStyles.kbd}>Esc</span></span>
            <button
              className={`${btnStyles.btn} ${styles.footerButton} ${checkedCount === 0 ? btnStyles.btnPrimary : ''}`}
              onClick={() => void runPredict(runModels, reviewRoots, true)}
              disabled={running || runModels.size === 0 || reviewItemIds.length === 0}
              type="button"
            >{running ? 'Running…' : 'Run'}</button>
            {checkedCount > 0 && <button className={`${btnStyles.btn} ${btnStyles.btnPrimary} ${styles.footerButton}`} onClick={() => void applyChecked()} disabled={applying || running} type="button">{applying ? 'Applying…' : `Apply ${checkedCount} ${checkedCount === 1 ? 'tag' : 'tags'}`}</button>}
          </div>
        </>
      }
    >
      <div className={styles.panelBody}>
        {running && <div className={styles.progressHairline}><ProgressBar done={progress.done} total={progress.total} height={2} /></div>}
        <div className={`${styles.sidebar} ${!showSidebar ? styles.sidebarHidden : ''}`}>
          <button type="button" className={`${styles.sidebarItem} ${viewMode === 'suggested' ? styles.sidebarItemActive : ''}`} onClick={() => setViewMode('suggested')}>
            <span className={styles.sidebarDot} style={{ background: 'var(--color-primary)' }} /><span className={styles.sidebarName}>Suggested</span><span className={styles.sidebarBadge}>{suggested.length}</span>
          </button>
          <button type="button" className={`${styles.sidebarItem} ${viewMode === 'below' ? styles.sidebarItemActive : ''}`} onClick={() => setViewMode('below')}>
            <span className={styles.sidebarDot} style={{ background: 'var(--color-strong-overlay)' }} /><span className={styles.sidebarName}>Below cutoff</span><span className={styles.sidebarBadge}>{below.length}</span>
          </button>
          {models.length > 0 && <div className={styles.sidebarSep} />}
          {models.map((model) => {
            const active = runModels.has(model.slug);
            return (
              <KbdTooltip key={model.slug} label={model.downloaded ? model.dataset : `${model.label} is not downloaded — get it in Settings`}><button type="button" className={`${styles.sidebarItem} ${active ? styles.sidebarItemSelected : ''} ${!model.downloaded ? styles.sidebarItemDisabled : ''}`} onClick={model.downloaded && !running ? () => toggleModel(model.slug) : undefined} disabled={!model.downloaded || running}>
                <div className={`${shellStyles.checkBox} ${active ? shellStyles.checkBoxChecked : ''}`}>{active && <IconCheck size={10} />}</div>
                <span className={styles.sidebarName}>{model.label}</span>
                <span className={styles.sidebarBadge}>{model.downloaded ? modelCounts.get(model.slug) || '·' : '·'}</span>
              </button></KbdTooltip>
            );
          })}
        </div>

        <section className={styles.reviewPane} aria-label="Media review">
          <div className={styles.reviewNavigation}>
            <KbdTooltip label="Previous item" shortcutId="view.prevImage">
              <button type="button" className={styles.navButton} aria-label="Previous item" onClick={() => moveActive(-1)} disabled={activeIndex <= 0}><IconChevronLeft size={15} /></button>
            </KbdTooltip>
            <span className={styles.reviewCounter}>{currentNumber} / {reviewItemIds.length}</span>
            <KbdTooltip label="Next item" shortcutId="view.nextImage">
              <button type="button" className={styles.navButton} aria-label="Next item" onClick={() => moveActive(1)} disabled={activeIndex >= reviewItemIds.length - 1}><IconChevronRight size={15} /></button>
            </KbdTooltip>
          </div>
          <div className={inspectorStyles.preview}>
            <div className={inspectorStyles.previewFrame} style={{ background: labToHex(activeMedia?.facts.palette[0]) ?? undefined }}>
              {activeMedia ? <ThumbnailImage src={mediaThumbnailUrl(activeMedia.facts.content_hash)} alt="" className={inspectorStyles.previewImage} draggable={false} /> : <div className={styles.previewEmpty}>Preparing preview…</div>}
              <div className={inspectorStyles.previewGlass} />
              {activeIsRunning && <div className={styles.previewStatus}>Analyzing this item…</div>}
            </div>
          </div>
          <div className={styles.mediaName}>{activeRoot?.label || `Item ${currentNumber || 1}`}</div>
          <div className={styles.mediaMeta}>{activePrediction ? `${activePrediction.predictions.length} tag predictions` : running ? 'Waiting for analysis' : 'No prediction result'}</div>
          <div className={styles.thumbnailRail} aria-label="Selected items">
            {reviewRoots.map((root, index) => (
              <KbdTooltip key={root.rootItemId} label={`Review item ${index + 1}`}><button type="button" className={`${styles.thumbnailButton} ${root.rootItemId === activeItemId ? styles.thumbnailButtonActive : ''}`} onClick={() => setActiveItemId(root.rootItemId)} aria-label={`Review item ${index + 1}`}>
                <ThumbnailImage src={mediaThumbnailUrl(root.previewMedia.facts.content_hash)} alt="" draggable={false} />
              </button></KbdTooltip>
            ))}
          </div>
        </section>

        <div className={styles.content}>
          {error && visibleTags.length > 0 && <div className={styles.partialError}>{error}</div>}
          <div className={styles.tagListScroller}>
            {error && visibleTags.length === 0 && !running ? <div className={styles.emptyState}><span className={styles.errorText}>{error}</span></div>
              : itemIds.length === 0 ? <div className={styles.emptyState}>Select specific library items to auto tag</div>
                : visibleTags.length === 0 ? <div className={styles.emptyState}>{activeIsRunning ? 'Analyzing this item…' : running ? 'Waiting for this item…' : runModels.size === 0 ? 'Select at least one downloaded model' : viewMode === 'below' ? 'Nothing below the cutoff' : predictions.length === 0 ? 'Choose models, then press Run' : 'No suggestions'}</div>
                  : visibleTags.map((tag) => {
                    const itemId = activeItemId ?? activePrediction?.rootId;
                    if (itemId == null) return null;
                    const checked = isChecked(itemId, tag);
                    const evidence = [...tag.models]
                      .sort((a, b) => b.confidence - a.confidence)
                      .map((entry) => modelLabel(entry.slug))
                      .join(', ');
                    return (
                      <button type="button" key={tag.key} className={`${styles.tagRow} ${checked ? styles.tagRowSelected : ''}`} onClick={() => setOverrides((previous) => new Map(previous).set(overrideKey(itemId, tag.key), !checked))}>
                        <div className={`${shellStyles.checkBox} ${checked ? shellStyles.checkBoxChecked : ''}`}>{checked && <IconCheck size={10} />}</div>
                        <IconBookmark className={styles.tagBookmark} style={{ '--tag-color': tagGroupColor(tag.namespace) } as React.CSSProperties} />
                        <span className={styles.tagName}>
                          {tag.namespace && tag.namespace !== 'general' && tag.namespace !== 'default' && <span className={styles.tagNamespace}>{tag.namespace}:</span>}
                          <span>{tag.subtag}</span>
                        </span>
                        <span className={styles.tagSeparator} aria-hidden="true">·</span>
                        <span className={styles.tagEvidence}>{evidence}</span>
                        <span className={styles.confPct}>{Math.round(tag.confidence * 100)}%</span>
                      </button>
                    );
                  })}
          </div>
        </div>
      </div>
    </OverlayShell>
  );
}
