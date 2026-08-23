/**
 * AI review portal.
 *
 * Predictions are read-only until the user applies them. Logical item IDs
 * cross the IPC boundary; physical file hashes never do.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconCheck, IconLayoutSidebar, IconSearch } from '@tabler/icons-react';
import { OverlayShell } from '../../shared/ui/OverlayShell';
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
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';
import styles from './AiTaggerPanel.module.css';

const NS_COLORS: Record<string, [number, number, number]> = {
  creator: [170, 0, 0], studio: [128, 0, 0], character: [0, 170, 0],
  person: [0, 128, 0], series: [170, 0, 170], species: [0, 130, 170],
  meta: [160, 160, 160], system: [153, 101, 21], rating: [153, 101, 21],
  '': [114, 160, 193], default: [114, 160, 193], general: [114, 160, 193],
};

function nsColor(namespace: string): string {
  const [r, g, b] = NS_COLORS[namespace.toLowerCase()] ?? NS_COLORS.default;
  return `rgb(${r}, ${g}, ${b})`;
}

type ViewMode = 'suggested' | 'below';

interface AggregatedTag {
  key: string;
  namespace: string;
  subtag: string;
  maxConf: number;
  models: string[];
  perItem: Map<number, number>;
}

function aggregate(predictions: MediaPrediction[], runModels: Set<string>): AggregatedTag[] {
  const byKey = new Map<string, AggregatedTag>();
  for (const item of predictions) {
    for (const prediction of item.predictions as AiTagPrediction[]) {
      if (!runModels.has(prediction.model)) continue;
      const key = prediction.namespace ? `${prediction.namespace}:${prediction.tag}` : prediction.tag;
      const aggregate = byKey.get(key) ?? {
        key,
        namespace: prediction.namespace,
        subtag: prediction.tag,
        maxConf: 0,
        models: [],
        perItem: new Map<number, number>(),
      };
      aggregate.maxConf = Math.max(aggregate.maxConf, prediction.confidence);
      if (!aggregate.models.includes(prediction.model)) aggregate.models.push(prediction.model);
      const previous = aggregate.perItem.get(item.mediaItemId) ?? 0;
      if (prediction.confidence > previous) aggregate.perItem.set(item.mediaItemId, prediction.confidence);
      byKey.set(key, aggregate);
    }
  }
  return [...byKey.values()].sort((a, b) => b.maxConf - a.maxConf);
}

export function AiTaggerPanel() {
  const portalState = useAtomValue(aiTaggerPortalAtom);
  const setPortalState = useSetAtom(aiTaggerPortalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const open = portalState.open;
  const anchorPosition = portalState.anchor ?? null;

  const [pinned, setPinned] = useState(false);
  const [showSidebar, setShowSidebar] = useState(true);
  const [query, setQuery] = useState('');
  const [models, setModels] = useState<AiModelStatus[]>([]);
  const [runModels, setRunModels] = useState<Set<string>>(new Set());
  const [predictions, setPredictions] = useState<MediaPrediction[]>([]);
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

  const itemIds = useMemo(
    () => (target?.kind === 'explicit' ? target.item_ids : []),
    [target],
  );
  const multi = itemIds.length > 1;
  const itemFingerprint = itemIds.join('\n');

  const closePortal = useCallback(() => {
    runGenerationRef.current += 1;
    runningRef.current = false;
    setRunning(false);
    setPortalState({ open: false });
  }, [setPortalState]);

  const runPredict = useCallback((slugs: Set<string>, ids: number[]) => {
    if (runningRef.current || slugs.size === 0 || ids.length === 0) return;
    const generation = ++runGenerationRef.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    void aiTagPredict(ids, [...slugs])
      .then((output) => {
        if (generation !== runGenerationRef.current) return;
        ranModelsRef.current = new Set([...ranModelsRef.current, ...slugs]);
        setPredictions(output.predictions);
        setThresholds(output.thresholds);
        const failures = output.predictions.filter((prediction) => prediction.error);
        if (failures.length > 0) {
          setError(`${failures.length} of ${output.predictions.length} media items could not be tagged. ${failures[0].error}`);
        }
      })
      .catch((reason) => {
        if (generation === runGenerationRef.current) setError(String(reason));
      })
      .finally(() => {
        if (generation === runGenerationRef.current) {
          runningRef.current = false;
          setRunning(false);
        }
      });
  }, []);

  useEffect(() => {
    if (!open) return;
    setQuery('');
    setPredictions([]);
    setOverrides(new Map());
    setViewMode('suggested');
    setError(null);
    ranModelsRef.current = new Set();
    void aiTaggerStatus()
      .then((status) => {
        setModels(status.models);
        setThresholds(status.thresholds);
        const ready = status.models.filter((model) => model.enabled && model.downloaded).map((model) => model.slug);
        const slugs = new Set(ready);
        setRunModels(slugs);
        runPredict(slugs, itemIds);
      })
      .catch((reason) => setError(String(reason)));
    const focusTimer = setTimeout(() => searchRef.current?.focus(), 50);
    return () => {
      clearTimeout(focusTimer);
      runGenerationRef.current += 1;
      runningRef.current = false;
    };
  }, [open, itemFingerprint, runPredict]);

  const toggleModel = useCallback((slug: string) => {
    setRunModels((previous) => {
      const next = new Set(previous);
      if (next.has(slug)) next.delete(slug);
      else {
        next.add(slug);
        if (!runningRef.current && !ranModelsRef.current.has(slug)) runPredict(next, itemIds);
      }
      return next;
    });
  }, [itemIds, runPredict]);

  const aggregated = useMemo(() => aggregate(predictions, runModels), [predictions, runModels]);
  const thresholdFor = useCallback((namespace: string) => thresholds[namespace] ?? thresholds.general ?? 0.35, [thresholds]);
  const suggested = useMemo(() => aggregated.filter((tag) => tag.maxConf >= thresholdFor(tag.namespace)), [aggregated, thresholdFor]);
  const below = useMemo(() => aggregated.filter((tag) => tag.maxConf < thresholdFor(tag.namespace)), [aggregated, thresholdFor]);
  const isChecked = useCallback((tag: AggregatedTag) => overrides.get(tag.key) ?? (tag.maxConf >= thresholdFor(tag.namespace) && tag.namespace !== 'rating'), [overrides, thresholdFor]);
  const checkedTags = useMemo(() => aggregated.filter(isChecked), [aggregated, isChecked]);

  const applyChecked = useCallback(async () => {
    if (checkedTags.length === 0 || itemIds.length === 0) return;
    const tagsByItem = new Map<number, string[]>();
    for (const tag of checkedTags) {
      const cutoff = thresholdFor(tag.namespace);
      for (const [itemId, confidence] of tag.perItem) {
        if (tag.maxConf < cutoff && confidence < cutoff) continue;
        const tags = tagsByItem.get(itemId) ?? [];
        tags.push(tag.key);
        tagsByItem.set(itemId, tags);
      }
    }
    setApplying(true);
    setError(null);
    try {
      await aiTagApply([...tagsByItem].map(([mediaItemId, tags]) => ({ media_item_id: mediaItemId, tags })));
      setOverrides(new Map());
      if (!pinned) setPortalState({ open: false });
    } catch (reason) {
      setError(String(reason));
    } finally {
      setApplying(false);
    }
  }, [checkedTags, itemIds.length, pinned, setPortalState, thresholdFor]);

  const visibleTags = useMemo(() => {
    const source = viewMode === 'suggested' ? suggested : below;
    const search = query.trim().toLowerCase();
    return search ? source.filter((tag) => tag.key.toLowerCase().includes(search)) : source;
  }, [below, query, suggested, viewMode]);

  if (!open) return null;

  return (
    <OverlayShell
      open={open}
      onClose={closePortal}
      width={showSidebar ? 540 : 340}
      pinned={pinned}
      anchorPosition={anchorPosition}
      onPinnedChange={setPinned}
      header={
        <>
          <div className={shellStyles.searchRow} style={{ flex: 1 }}>
            <IconSearch size={14} className={shellStyles.searchIcon} />
            <input ref={searchRef} className={shellStyles.searchInput} placeholder="Filter suggestions..." value={query} onChange={(event) => setQuery(event.target.value)} />
          </div>
          {running && <span className={styles.runCounter}>Analyzing {itemIds.length} items…</span>}
          <button className={shellStyles.pinBtn} onClick={() => setShowSidebar((value) => !value)} type="button" title={showSidebar ? 'Hide sidebar' : 'Show sidebar'}>
            <IconLayoutSidebar size={14} />
          </button>
        </>
      }
      footer={
        <>
          <div className={styles.cutoff}>Using Settings thresholds</div>
          <div className={btnStyles.btnGroup}>
            <span className={shellStyles.kbdHint}><span className={shellStyles.kbd}>Esc</span></span>
            {checkedTags.length > 0 && <button className={`${btnStyles.btn} ${btnStyles.btnPrimary}`} onClick={() => void applyChecked()} disabled={applying} type="button">{applying ? 'Applying…' : `Apply ${checkedTags.length} ${checkedTags.length === 1 ? 'tag' : 'tags'}`}</button>}
          </div>
        </>
      }
    >
      <div className={styles.panelBody}>
        <div className={`${styles.sidebar} ${!showSidebar ? styles.sidebarHidden : ''}`}>
          <div className={`${styles.sidebarItem} ${viewMode === 'suggested' ? styles.sidebarItemActive : ''}`} onClick={() => setViewMode('suggested')}>
            <span className={styles.sidebarDot} style={{ background: 'var(--color-primary)' }} /><span className={styles.sidebarName}>Suggested</span><span className={styles.sidebarBadge}>{suggested.length}</span>
          </div>
          <div className={`${styles.sidebarItem} ${viewMode === 'below' ? styles.sidebarItemActive : ''}`} onClick={() => setViewMode('below')}>
            <span className={styles.sidebarDot} style={{ background: 'var(--color-strong-overlay)' }} /><span className={styles.sidebarName}>Below cutoff</span><span className={styles.sidebarBadge}>{below.length}</span>
          </div>
          {models.length > 0 && <div className={styles.sidebarSep} />}
          {models.map((model) => {
            const active = runModels.has(model.slug);
            return (
              <div key={model.slug} className={`${styles.sidebarItem} ${!model.downloaded ? styles.sidebarItemDisabled : ''}`} title={model.downloaded ? model.dataset : `${model.label} is not downloaded — get it in Settings`} onClick={model.downloaded && !running ? () => toggleModel(model.slug) : undefined}>
                <div className={`${shellStyles.checkBox} ${active ? shellStyles.checkBoxChecked : ''}`}>{active && <IconCheck size={10} />}</div>
                <span className={styles.sidebarName}>{model.label}</span>
                <span className={styles.sidebarBadge}>{model.downloaded ? aggregated.filter((tag) => tag.models.includes(model.slug)).length || '·' : '·'}</span>
              </div>
            );
          })}
        </div>
        <div className={styles.content}>
          {error && visibleTags.length > 0 && <div className={styles.partialError}>{error}</div>}
          <div className={styles.tagListScroller}>
            {error && visibleTags.length === 0 ? <div className={styles.emptyState}><span className={styles.errorText}>{error}</span></div>
              : itemIds.length === 0 ? <div className={styles.emptyState}>Select specific media items to auto tag</div>
                : visibleTags.length === 0 ? <div className={styles.emptyState}>{running ? 'Analyzing…' : runModels.size === 0 ? 'No models selected — enable one in Settings' : viewMode === 'below' ? 'Nothing below the cutoff' : 'No suggestions'}</div>
                  : visibleTags.map((tag) => {
                    const checked = isChecked(tag);
                    const showNamespace = tag.namespace !== '' && tag.namespace !== 'general';
                    const cutoff = thresholdFor(tag.namespace);
                    const matched = multi ? [...tag.perItem.values()].filter((confidence) => confidence >= cutoff).length : 0;
                    return <div key={tag.key} className={styles.tagRow} title={`${Math.round(tag.maxConf * 100)}% · ${tag.models.join(', ')}${multi ? ` · ${matched}/${itemIds.length} items` : ''}`} onClick={() => setOverrides((previous) => new Map(previous).set(tag.key, !checked))}>
                      <div className={`${shellStyles.checkBox} ${checked ? shellStyles.checkBoxChecked : ''}`}>{checked && <IconCheck size={10} />}</div>
                      <span className={styles.tagDot} style={{ background: nsColor(tag.namespace) }} /><span className={styles.tagName}>{showNamespace && <span className={styles.tagNs}>{tag.namespace}:</span>}{tag.subtag}</span><span className={styles.confPct}>{Math.round(tag.maxConf * 100)}%</span>
                    </div>;
                  })}
          </div>
        </div>
      </div>
    </OverlayShell>
  );
}
