/**
 * AiTaggerPanel — auto-tag portal.
 *
 * Same anatomy as TagSelectPanel: 540px OverlayShell with search header,
 * 200px sidebar + 340px content, 28px checkbox rows. Runs the enabled
 * downloaded models over the current selection on open, streams progress
 * via the auto_tag runtime task, and applies checked suggestions through
 * ai_tag_apply. Nothing is written until Apply.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconCheck, IconLayoutSidebar, IconSearch, IconX } from '@tabler/icons-react';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { ProgressBar } from '../../shared/ui/ProgressBar';
import { KbdTooltip } from '../../shared/ui/KbdTooltip';
import { aiTaggerPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import { useAiTaggerTasks } from '../../runtime/aiTaggerTasks';
import {
  aiTagApply,
  aiTagCancel,
  aiTagPredict,
  aiTaggerStatus,
  type AiTaggerModelStatus,
  type FilePrediction,
} from '../../platform/aiTaggerApi';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import btnStyles from '../../shared/styles/actionButton.module.css';
import styles from './AiTaggerPanel.module.css';

// Namespace → RGB color (same as TagChip / TagSelectPanel)
const NS_COLORS: Record<string, [number, number, number]> = {
  creator: [170, 0, 0], studio: [128, 0, 0], character: [0, 170, 0],
  person: [0, 128, 0], series: [170, 0, 170], species: [0, 130, 170],
  meta: [160, 160, 160], system: [153, 101, 21], rating: [153, 101, 21],
  '': [114, 160, 193], default: [114, 160, 193], general: [114, 160, 193],
};

function nsColor(ns: string): string {
  const [r, g, b] = NS_COLORS[(ns ?? '').toLowerCase()] ?? NS_COLORS.default;
  return `rgb(${r}, ${g}, ${b})`;
}

type ViewMode = 'suggested' | 'below';

const DEFAULT_THRESHOLDS: Record<string, number> = {
  general: 0.35,
  character: 0.35,
  copyright: 0.35,
  artist: 0.35,
  species: 0.35,
  rating: 0.35,
};

interface AggregatedTag {
  /** Canonical tag string, e.g. `character:hatsune_miku`. */
  key: string;
  namespace: string;
  subtag: string;
  /** Highest confidence across selected files. */
  maxConf: number;
  /** Model slugs that predicted this tag. */
  models: string[];
  /** Per-file best confidence. */
  perFile: Map<string, number>;
}

function aggregate(predictions: FilePrediction[], runModels: Set<string>): AggregatedTag[] {
  const byKey = new Map<string, AggregatedTag>();
  for (const file of predictions) {
    for (const pred of file.tags) {
      if (!runModels.has(pred.model)) continue;
      const key = pred.namespace ? `${pred.namespace}:${pred.tag}` : pred.tag;
      let agg = byKey.get(key);
      if (!agg) {
        agg = { key, namespace: pred.namespace, subtag: pred.tag, maxConf: 0, models: [], perFile: new Map() };
        byKey.set(key, agg);
      }
      agg.maxConf = Math.max(agg.maxConf, pred.confidence);
      if (!agg.models.includes(pred.model)) agg.models.push(pred.model);
      const prev = agg.perFile.get(file.hash) ?? 0;
      if (pred.confidence > prev) agg.perFile.set(file.hash, pred.confidence);
    }
  }
  return [...byKey.values()].sort((a, b) => b.maxConf - a.maxConf);
}

export function AiTaggerPanel() {
  const portalState = useAtomValue(aiTaggerPortalAtom);
  const setPortalState = useSetAtom(aiTaggerPortalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const { autoTag: runTask } = useAiTaggerTasks();

  const open = portalState.open;
  const anchorPosition = portalState.anchor ?? null;

  const [pinned, setPinned] = useState(false);
  const [showSidebar, setShowSidebar] = useState(true);
  const [query, setQuery] = useState('');
  const [models, setModels] = useState<AiTaggerModelStatus[]>([]);
  const [runModels, setRunModels] = useState<Set<string>>(new Set());
  const [predictions, setPredictions] = useState<FilePrediction[]>([]);
  const [running, setRunning] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [thresholds, setThresholds] = useState<Record<string, number>>(DEFAULT_THRESHOLDS);
  const [viewMode, setViewMode] = useState<ViewMode>('suggested');
  // User overrides on top of the default check state, keyed by tag.
  const [overrides, setOverrides] = useState<Map<string, boolean>>(new Map());

  const searchRef = useRef<HTMLInputElement>(null);
  const ranModelsRef = useRef<Set<string>>(new Set());
  const runningRef = useRef(false);
  const runGenerationRef = useRef(0);

  const hashes = useMemo(
    () => (target?.kind === 'entity_hashes' ? target.entity_hashes ?? [] : []),
    [target],
  );
  const multi = hashes.length > 1;
  const hashFingerprint = hashes.join('\n');

  const closePortal = useCallback(() => {
    runGenerationRef.current += 1;
    if (runningRef.current) void aiTagCancel().catch(() => {});
    runningRef.current = false;
    setPortalState({ open: false });
  }, [setPortalState]);

  const runPredict = useCallback((slugs: Set<string>, fileHashes: string[]) => {
    if (runningRef.current || slugs.size === 0 || fileHashes.length === 0) return;
    const generation = ++runGenerationRef.current;
    runningRef.current = true;
    setRunning(true);
    setError(null);
    aiTagPredict(fileHashes, [...slugs])
      .then((out) => {
        if (generation !== runGenerationRef.current) return;
        ranModelsRef.current = new Set([...ranModelsRef.current, ...slugs]);
        setPredictions(out.predictions);
        setThresholds(out.thresholds);
        const failures = out.predictions.filter((prediction) => prediction.error);
        if (failures.length > 0) {
          const first = failures[0].error ?? 'Prediction failed';
          setError(`${failures.length} of ${out.predictions.length} images could not be tagged. ${first}`);
        }
      })
      .catch((e) => {
        if (generation === runGenerationRef.current) setError(String(e));
      })
      .finally(() => {
        if (generation !== runGenerationRef.current) return;
        runningRef.current = false;
        setRunning(false);
      });
  }, []);

  // Reset + auto-run on open
  useEffect(() => {
    if (!open) return;
    setQuery('');
    setPredictions([]);
    setOverrides(new Map());
    setViewMode('suggested');
    setError(null);
    setApplying(false);
    ranModelsRef.current = new Set();
    void aiTaggerStatus()
      .then((status) => {
        setModels(status.models);
        const ready = status.models.filter((m) => m.enabled && m.downloaded).map((m) => m.slug);
        const slugs = new Set(ready);
        setRunModels(slugs);
        runPredict(slugs, hashes);
      })
      .catch((e) => setError(String(e)));
    setTimeout(() => searchRef.current?.focus(), 50);
    return () => {
      runGenerationRef.current += 1;
      if (runningRef.current) void aiTagCancel().catch(() => {});
      runningRef.current = false;
    };
  }, [open, hashFingerprint]); // eslint-disable-line react-hooks/exhaustive-deps

  const toggleModel = useCallback(
    (slug: string) => {
      setRunModels((prev) => {
        const next = new Set(prev);
        if (next.has(slug)) {
          next.delete(slug);
        } else {
          next.add(slug);
          // Newly ticked model hasn't run yet — re-run with the full set.
          if (!runningRef.current && !ranModelsRef.current.has(slug)) runPredict(next, hashes);
        }
        return next;
      });
    },
    [hashes, runPredict],
  );

  const aggregated = useMemo(() => aggregate(predictions, runModels), [predictions, runModels]);
  const thresholdFor = useCallback(
    (namespace: string) => thresholds[namespace] ?? thresholds.general ?? 0.35,
    [thresholds],
  );
  const suggested = useMemo(
    () => aggregated.filter((a) => a.maxConf >= thresholdFor(a.namespace)),
    [aggregated, thresholdFor],
  );
  const below = useMemo(
    () => aggregated.filter((a) => a.maxConf < thresholdFor(a.namespace)),
    [aggregated, thresholdFor],
  );

  const isChecked = useCallback(
    (agg: AggregatedTag) =>
      overrides.get(agg.key) ?? (agg.maxConf >= thresholdFor(agg.namespace) && agg.namespace !== 'rating'),
    [overrides, thresholdFor],
  );

  const toggleTag = useCallback(
    (agg: AggregatedTag) => {
      setOverrides((prev) => {
        const next = new Map(prev);
        next.set(
          agg.key,
          !(next.get(agg.key) ??
            (agg.maxConf >= thresholdFor(agg.namespace) && agg.namespace !== 'rating')),
        );
        return next;
      });
    },
    [thresholdFor],
  );

  const checkedTags = useMemo(() => aggregated.filter(isChecked), [aggregated, isChecked]);

  const applyChecked = useCallback(async () => {
    if (checkedTags.length === 0 || hashes.length === 0) return;
    // Each tag is written only to the files that cleared the cutoff for it;
    // manually rescued below-cutoff tags go to every file that predicted them.
    const tagsByFile = new Map<string, string[]>();
    for (const agg of checkedTags) {
      const cutoff = thresholdFor(agg.namespace);
      const eligible = [...agg.perFile.entries()]
        .filter(([, conf]) => (agg.maxConf >= cutoff ? conf >= cutoff : true))
        .map(([hash]) => hash);
      for (const hash of eligible) {
        const list = tagsByFile.get(hash);
        if (list) list.push(agg.key);
        else tagsByFile.set(hash, [agg.key]);
      }
    }
    const assignments = [...tagsByFile].map(([hash, tags]) => ({ hash, tags }));
    setApplying(true);
    setError(null);
    try {
      await aiTagApply(assignments);
      setOverrides(new Map());
      if (!pinned) setPortalState({ open: false });
    } catch (e) {
      setError(String(e));
    } finally {
      setApplying(false);
    }
  }, [checkedTags, hashes, thresholdFor, pinned, setPortalState]);

  const visibleTags = useMemo(() => {
    const source = viewMode === 'suggested' ? suggested : below;
    const q = query.trim().toLowerCase();
    if (!q) return source;
    return source.filter((a) => a.key.toLowerCase().includes(q));
  }, [viewMode, suggested, below, query]);

  const modelTagCount = useCallback(
    (slug: string) => aggregated.filter((a) => a.models.includes(slug)).length,
    [aggregated],
  );

  const taskRunning = runTask != null && (runTask.status === 'running' || runTask.status === 'cancelling');
  const progressDone = Number(runTask?.progress?.done ?? 0);
  const progressTotal = Number(runTask?.progress?.total ?? hashes.length);

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
            <input
              ref={searchRef}
              className={shellStyles.searchInput}
              placeholder="Filter suggestions..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
          {(running || taskRunning) && (
            <>
              <span className={styles.runCounter}>
                {progressDone} / {progressTotal}
              </span>
              <KbdTooltip label="Cancel run">
                <button
                  className={shellStyles.pinBtn}
                  onClick={() => void aiTagCancel().catch(() => {})}
                  type="button"
                >
                  <IconX size={14} />
                </button>
              </KbdTooltip>
            </>
          )}
          <button
            className={shellStyles.pinBtn}
            onClick={() => setShowSidebar((v) => !v)}
            type="button"
            title={showSidebar ? 'Hide sidebar' : 'Show sidebar'}
          >
            <IconLayoutSidebar size={14} />
          </button>
        </>
      }
      footer={
        <>
          <div className={styles.cutoff}>Using Settings thresholds</div>
          <div className={btnStyles.btnGroup}>
            <span className={shellStyles.kbdHint}><span className={shellStyles.kbd}>Esc</span></span>
            {checkedTags.length > 0 && (
              <button
                className={`${btnStyles.btn} ${btnStyles.btnPrimary}`}
                onClick={() => void applyChecked()}
                disabled={applying}
                type="button"
              >
                {applying
                  ? 'Applying…'
                  : `Apply ${checkedTags.length} ${checkedTags.length === 1 ? 'tag' : 'tags'}`}
              </button>
            )}
          </div>
        </>
      }
    >
      <div className={styles.panelBody}>
        {(running || taskRunning) && (
          <div className={styles.progressHairline}>
            <ProgressBar done={progressDone} total={Math.max(progressTotal, 1)} height={2} />
          </div>
        )}

        {/* Sidebar */}
        <div className={`${styles.sidebar} ${!showSidebar ? styles.sidebarHidden : ''}`}>
          <div
            className={`${styles.sidebarItem} ${viewMode === 'suggested' ? styles.sidebarItemActive : ''}`}
            onClick={() => setViewMode('suggested')}
          >
            <span className={styles.sidebarDot} style={{ background: 'var(--color-primary)' }} />
            <span className={styles.sidebarName}>Suggested</span>
            <span className={styles.sidebarBadge}>{suggested.length}</span>
          </div>
          <div
            className={`${styles.sidebarItem} ${viewMode === 'below' ? styles.sidebarItemActive : ''}`}
            onClick={() => setViewMode('below')}
          >
            <span className={styles.sidebarDot} style={{ background: 'var(--color-strong-overlay)' }} />
            <span className={styles.sidebarName}>Below cutoff</span>
            <span className={styles.sidebarBadge}>{below.length}</span>
          </div>
          {models.length > 0 && <div className={styles.sidebarSep} />}
          {models.map((m) => {
            const active = runModels.has(m.slug);
            return (
              <div
                key={m.slug}
                className={`${styles.sidebarItem} ${!m.downloaded ? styles.sidebarItemDisabled : ''}`}
                title={m.downloaded ? m.dataset : `${m.label} is not downloaded — get it in Settings`}
                onClick={m.downloaded && !running ? () => toggleModel(m.slug) : undefined}
              >
                <div className={`${shellStyles.checkBox} ${active ? shellStyles.checkBoxChecked : ''}`}>
                  {active && <IconCheck size={10} />}
                </div>
                <span className={styles.sidebarName}>{m.label}</span>
                <span className={styles.sidebarBadge}>
                  {m.downloaded ? modelTagCount(m.slug) || '·' : '·'}
                </span>
              </div>
            );
          })}
        </div>

        {/* Content */}
        <div className={styles.content}>
          {error && visibleTags.length > 0 && (
            <div className={styles.partialError}>{error}</div>
          )}
          <div className={styles.tagListScroller}>
            {error && visibleTags.length === 0 ? (
              <div className={styles.emptyState}>
                <span className={styles.errorText}>{error}</span>
              </div>
            ) : hashes.length === 0 ? (
              <div className={styles.emptyState}>Select images to auto tag</div>
            ) : visibleTags.length === 0 ? (
              <div className={styles.emptyState}>
                {running || taskRunning
                  ? 'Analyzing…'
                  : runModels.size === 0
                    ? 'No models selected — tick a model in the sidebar, or enable one in Settings'
                    : viewMode === 'below'
                      ? 'Nothing below the cutoff'
                      : 'No suggestions'}
              </div>
            ) : (
              visibleTags.map((agg) => {
                const checked = isChecked(agg);
                const showNs = agg.namespace !== '' && agg.namespace !== 'general';
                const cutoff = thresholdFor(agg.namespace);
                const matched = multi
                  ? [...agg.perFile.values()].filter((c) => c >= cutoff).length
                  : 0;
                const tooltip = `${Math.round(agg.maxConf * 100)}% · ${agg.models.join(', ')}${
                  multi ? ` · ${matched}/${hashes.length} images` : ''
                }`;
                return (
                  <div
                    key={agg.key}
                    className={styles.tagRow}
                    title={tooltip}
                    onClick={() => toggleTag(agg)}
                  >
                    <div className={`${shellStyles.checkBox} ${checked ? shellStyles.checkBoxChecked : ''}`}>
                      {checked && <IconCheck size={10} />}
                    </div>
                    <span className={styles.tagDot} style={{ background: nsColor(agg.namespace) }} />
                    <span className={styles.tagName}>
                      {showNs && <span className={styles.tagNs}>{agg.namespace}:</span>}
                      {agg.subtag}
                    </span>
                    <span className={styles.confPct}>{Math.round(agg.maxConf * 100)}%</span>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>
    </OverlayShell>
  );
}
