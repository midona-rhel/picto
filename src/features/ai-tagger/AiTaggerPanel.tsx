/**
 * AI review portal.
 *
 * Predictions are reviewed per media item and remain read-only until applied.
 * Logical item IDs drive mutations; hashes are used only to render previews.
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
  type AiModelStatus,
  type AiTagPrediction,
  type MediaPrediction,
} from '../../platform/aiTaggerApi';
import { viewerController } from '../../controllers/viewerController';
import type { MediaDetails } from '../../shared/types/generated/application/MediaDetails';
import { mediaThumbnailUrl } from '../../shared/lib/mediaUrl';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';
import inspectorStyles from '../inspector/Inspector.module.css';
import { tagGroupColor } from '../tags/tagGroupPresentation';
import styles from './AiTaggerPanel.module.css';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';

type ViewMode = 'suggested' | 'below';

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

function predictionTags(prediction: MediaPrediction | undefined, runModels: Set<string>): ReviewTag[] {
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

function mergePredictionResults(previous: MediaPrediction[], incoming: MediaPrediction[], replacedModels: Set<string>): MediaPrediction[] {
  const byItem = new Map(previous.map((entry) => [entry.mediaItemId, entry]));
  for (const next of incoming) {
    const current = byItem.get(next.mediaItemId);
    byItem.set(next.mediaItemId, {
      mediaItemId: next.mediaItemId,
      predictions: [
        ...(current?.predictions ?? []).filter((tag) => !replacedModels.has(tag.model)),
        ...next.predictions,
      ],
      error: next.error,
    });
  }
  return [...byItem.values()];
}

function uniqueMedia(details: MediaDetails[]): MediaDetails[] {
  const seen = new Set<number>();
  return details.filter((media) => {
    if (seen.has(media.media_item_id)) return false;
    seen.add(media.media_item_id);
    return true;
  });
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
  const [predictions, setPredictions] = useState<MediaPrediction[]>([]);
  const [reviewMedia, setReviewMedia] = useState<MediaDetails[]>([]);
  const [activeItemId, setActiveItemId] = useState<number | null>(null);
  const [progress, setProgress] = useState<RunProgress>({ done: 0, total: 0, currentItemId: null });
  const [backend, setBackend] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [thresholds, setThresholds] = useState<Record<string, number>>({ general: 0.35 });
  const [viewMode, setViewMode] = useState<ViewMode>('suggested');
  const [overrides, setOverrides] = useState<Map<string, boolean>>(new Map());
  const searchRef = useRef<HTMLInputElement>(null);
  const runGenerationRef = useRef(0);
  const runningRef = useRef(false);
  const ranModelsRef = useRef<Set<string>>(new Set());

  const itemIds = useMemo(() => (target?.kind === 'explicit' ? target.item_ids : []), [target]);
  const itemFingerprint = itemIds.join('\n');

  const closePortal = useCallback(() => {
    runGenerationRef.current += 1;
    runningRef.current = false;
    setRunning(false);
    setPortalState({ open: false });
  }, [setPortalState]);

  const runPredict = useCallback(async (slugs: Set<string>, ids: number[], reset = false) => {
    if (runningRef.current || slugs.size === 0 || ids.length === 0) return;
    const generation = ++runGenerationRef.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    setProgress({ done: 0, total: ids.length, currentItemId: ids[0] ?? null });
    if (reset) setPredictions([]);
    const failures: string[] = [];

    try {
      for (let index = 0; index < ids.length; index += 1) {
        const itemId = ids[index];
        if (generation !== runGenerationRef.current) return;
        setProgress({ done: index, total: ids.length, currentItemId: itemId });
        try {
          const output = await aiTagPredict([itemId], [...slugs]);
          if (generation !== runGenerationRef.current) return;
          setThresholds(output.thresholds);
          setPredictions((previous) => mergePredictionResults(previous, output.predictions, slugs));
          setActiveItemId((current) => current ?? output.predictions[0]?.mediaItemId ?? null);
          for (const failed of output.predictions.filter((entry) => entry.error)) {
            failures.push(`Item ${failed.mediaItemId}: ${failed.error}`);
          }
        } catch (reason) {
          failures.push(`Item ${itemId}: ${String(reason)}`);
        }
        setProgress({ done: index + 1, total: ids.length, currentItemId: itemId });
      }
      ranModelsRef.current = new Set([...ranModelsRef.current, ...slugs]);
      if (failures.length > 0) {
        setError(`${failures.length} of ${ids.length} media items could not be tagged. ${failures[0]}`);
      }
      try {
        const status = await aiTaggerStatus();
        if (generation === runGenerationRef.current) setBackend(status.cachedBackend ?? 'CPU');
      } catch {
        if (generation === runGenerationRef.current) setBackend((current) => current ?? 'CPU');
      }
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
    setReviewMedia([]);
    setActiveItemId(null);
    setOverrides(new Map());
    setViewMode('suggested');
    setError(null);
    setBackend(null);
    setProgress({ done: 0, total: 0, currentItemId: null });
    ranModelsRef.current = new Set();

    void Promise.all([
      aiTaggerStatus(),
      Promise.allSettled(itemIds.map((itemId) => viewerController.getItemDetails(itemId))),
    ]).then(([status, detailResults]) => {
      if (generation !== runGenerationRef.current) return;
      const media = uniqueMedia(detailResults.flatMap((result) => result.status === 'fulfilled' ? result.value.media : []));
      const detailFailures = detailResults.filter((result) => result.status === 'rejected');
      setModels(status.models);
      setThresholds(status.thresholds);
      setBackend(status.cachedBackend);
      setReviewMedia(media);
      setActiveItemId(media[0]?.media_item_id ?? null);
      if (detailFailures.length > 0) setError(`${detailFailures.length} selected items could not be prepared for AI review.`);
      const ready = status.models.filter((model) => model.enabled && model.downloaded).map((model) => model.slug);
      const slugs = new Set(ready);
      setRunModels(slugs);
      void runPredict(slugs, media.map((entry) => entry.media_item_id), true);
    }).catch((reason) => {
      if (generation === runGenerationRef.current) setError(String(reason));
    });

    const focusTimer = setTimeout(() => searchRef.current?.focus(), 50);
    return () => {
      clearTimeout(focusTimer);
      runGenerationRef.current += 1;
      runningRef.current = false;
    };
  }, [open, itemFingerprint, runPredict]);

  const reviewItemIds = useMemo(() => reviewMedia.length > 0
    ? reviewMedia.map((media) => media.media_item_id)
    : predictions.map((prediction) => prediction.mediaItemId), [predictions, reviewMedia]);
  const activeIndex = Math.max(0, reviewItemIds.indexOf(activeItemId ?? reviewItemIds[0]));
  const activeMedia = reviewMedia.find((media) => media.media_item_id === activeItemId) ?? reviewMedia[activeIndex] ?? null;
  const activePrediction = predictions.find((prediction) => prediction.mediaItemId === activeItemId);

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
    else {
      next.add(slug);
      if (!runningRef.current && !ranModelsRef.current.has(slug)) void runPredict(new Set([slug]), reviewItemIds);
    }
    setRunModels(next);
  }, [reviewItemIds, runModels, runPredict]);

  const thresholdFor = useCallback((namespace: string) => thresholds[namespace] ?? thresholds.general ?? 0.35, [thresholds]);
  const activeTags = useMemo(() => predictionTags(activePrediction, runModels), [activePrediction, runModels]);
  const suggested = useMemo(() => activeTags.filter((tag) => tag.confidence >= thresholdFor(tag.namespace)), [activeTags, thresholdFor]);
  const below = useMemo(() => activeTags.filter((tag) => tag.confidence < thresholdFor(tag.namespace)), [activeTags, thresholdFor]);
  const isChecked = useCallback((itemId: number, tag: ReviewTag) => (
    overrides.get(overrideKey(itemId, tag.key))
      ?? tag.confidence >= thresholdFor(tag.namespace)
  ), [overrides, thresholdFor]);

  const assignments = useMemo(() => predictions.flatMap((prediction) => {
    const tags = predictionTags(prediction, runModels)
      .filter((tag) => isChecked(prediction.mediaItemId, tag))
      .map((tag) => tag.key);
    return tags.length > 0 ? [{ media_item_id: prediction.mediaItemId, tags }] : [];
  }), [isChecked, predictions, runModels]);
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
        if (tag.model === model.slug) keys.add(`${prediction.mediaItemId}\u0000${tag.namespace}:${tag.tag}`);
      }
    }
    return [model.slug, keys.size];
  })), [models, predictions]);

  if (!open) return null;

  const currentNumber = reviewItemIds.length > 0 ? activeIndex + 1 : 0;
  const activeIsRunning = running && progress.currentItemId === activeItemId;

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
          <div className={styles.cutoff}>{backend ? `${backend} inference` : 'Local inference'} · Settings thresholds</div>
          <div className={btnStyles.btnGroup}>
            <span className={shellStyles.kbdHint}><span className={shellStyles.kbd}>Esc</span></span>
            {checkedCount > 0 && <button className={`${btnStyles.btn} ${btnStyles.btnPrimary} ${styles.applyButton}`} onClick={() => void applyChecked()} disabled={applying || running} type="button">{applying ? 'Applying…' : `Apply ${checkedCount} ${checkedCount === 1 ? 'tag' : 'tags'}`}</button>}
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
            <KbdTooltip label="Previous image" shortcutId="view.prevImage">
              <button type="button" className={styles.navButton} aria-label="Previous image" onClick={() => moveActive(-1)} disabled={activeIndex <= 0}><IconChevronLeft size={15} /></button>
            </KbdTooltip>
            <span className={styles.reviewCounter}>{currentNumber} / {reviewItemIds.length}</span>
            <KbdTooltip label="Next image" shortcutId="view.nextImage">
              <button type="button" className={styles.navButton} aria-label="Next image" onClick={() => moveActive(1)} disabled={activeIndex >= reviewItemIds.length - 1}><IconChevronRight size={15} /></button>
            </KbdTooltip>
          </div>
          <div className={inspectorStyles.preview}>
            <div className={inspectorStyles.previewFrame} style={{ background: activeMedia?.dominant_color_hex ?? undefined }}>
              {activeMedia ? <ThumbnailImage src={mediaThumbnailUrl(activeMedia.file_hash)} alt="" className={inspectorStyles.previewImage} draggable={false} /> : <div className={styles.previewEmpty}>Preparing preview…</div>}
              <div className={inspectorStyles.previewGlass} />
              {activeIsRunning && <div className={styles.previewStatus}>Analyzing this image…</div>}
            </div>
          </div>
          <div className={styles.mediaName}>{activeMedia?.name || `Image ${currentNumber || 1}`}</div>
          <div className={styles.mediaMeta}>{activePrediction ? `${activePrediction.predictions.length} tag predictions` : running ? 'Waiting for analysis' : 'No prediction result'}</div>
          <div className={styles.thumbnailRail} aria-label="Selected images">
            {reviewMedia.map((media, index) => (
              <KbdTooltip key={media.media_item_id} label={`Review image ${index + 1}`}><button type="button" className={`${styles.thumbnailButton} ${media.media_item_id === activeItemId ? styles.thumbnailButtonActive : ''}`} onClick={() => setActiveItemId(media.media_item_id)} aria-label={`Review image ${index + 1}`}>
                <ThumbnailImage src={mediaThumbnailUrl(media.file_hash)} alt="" draggable={false} />
              </button></KbdTooltip>
            ))}
          </div>
        </section>

        <div className={styles.content}>
          {error && visibleTags.length > 0 && <div className={styles.partialError}>{error}</div>}
          <div className={styles.tagListScroller}>
            {error && visibleTags.length === 0 && !running ? <div className={styles.emptyState}><span className={styles.errorText}>{error}</span></div>
              : itemIds.length === 0 ? <div className={styles.emptyState}>Select specific media items to auto tag</div>
                : visibleTags.length === 0 ? <div className={styles.emptyState}>{activeIsRunning ? 'Analyzing this image…' : running ? 'Waiting for this image…' : runModels.size === 0 ? 'No models selected — enable one in Settings' : viewMode === 'below' ? 'Nothing below the cutoff' : 'No suggestions'}</div>
                  : visibleTags.map((tag) => {
                    const itemId = activeItemId ?? activePrediction?.mediaItemId;
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
