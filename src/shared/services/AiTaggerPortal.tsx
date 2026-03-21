import { useState, useEffect, useCallback, useRef, useLayoutEffect, useMemo } from 'react';
import { Checkbox, Loader } from '@mantine/core';
import { IconPin, IconPinFilled, IconSparkles, IconCheck } from '@tabler/icons-react';
import { TextButton } from '#ui/TextButton';
import { api } from '#desktop/api';
import { OverlayShell } from '#ui/OverlayShell';
import { NamespaceTagChip } from '#ui/NamespaceTagChip';
import { registerAiTaggerOpenHandler, type AiTaggerRequest } from './aiTaggerService';
import type { AiTagPrediction } from '../types/api';
import st from './AiTaggerPortal.module.css';

const WD14_SLUG = 'wd14-swinv2-v3';
const E621_SLUG = 'z3d-e621-convnext';

const NAMESPACE_ORDER = ['general', 'character', 'copyright', 'artist', 'species', 'rating'];

/** Format a tag for storage — "general" tags are unnamespaced, others get "ns:tag". */
function formatTagForApply(namespace: string, tag: string): string {
  return namespace === 'general' || namespace === '' ? tag : `${namespace}:${tag}`;
}

/** Internal key for dedup — always includes namespace for uniqueness. */
function tagKey(namespace: string, tag: string): string {
  return `${namespace}:${tag}`;
}
const NAMESPACE_LABELS: Record<string, string> = {
  general: 'General',
  character: 'Character',
  copyright: 'Copyright / Series',
  artist: 'Artist',
  species: 'Species',
  rating: 'Rating',
};

export function AiTaggerPortal() {
  const [request, setRequest] = useState<AiTaggerRequest | null>(null);
  const [openKey, setOpenKey] = useState(0);

  useEffect(() => {
    return registerAiTaggerOpenHandler((req) => {
      setOpenKey((k) => k + 1);
      setRequest(req);
    });
  }, []);

  const handleClose = useCallback(() => setRequest(null), []);

  if (!request) return null;

  return (
    <AiTaggerPanel
      key={openKey}
      anchorEl={request.anchorEl}
      anchorPoint={request.anchorPoint}
      hashes={request.hashes}
      onApply={request.onApply}
      onClose={handleClose}
    />
  );
}

function AiTaggerPanel({
  anchorEl,
  anchorPoint,
  hashes,
  onApply,
  onClose,
}: {
  anchorEl: HTMLElement;
  anchorPoint?: { x: number; y: number };
  hashes: string[];
  onApply: (tags: string[]) => Promise<void>;
  onClose: () => void;
}) {
  const [pinned, setPinned] = useState(false);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  // Model availability — check downloaded status only (user can run even if not "enabled" in settings)
  const [wd14Available, setWd14Available] = useState(false);
  const [e621Available, setE621Available] = useState(false);

  useEffect(() => {
    void api.aiTagger.status().then((st) => {
      const wd14 = st.models.find((m) => m.slug.startsWith('wd14'));
      const e621 = st.models.find((m) => m.slug.startsWith('z3d-e621'));
      setWd14Available(wd14?.downloaded === true);
      setE621Available(e621?.downloaded === true);
    });
  }, []);

  // Model run state
  const [wd14Running, setWd14Running] = useState(false);
  const [e621Running, setE621Running] = useState(false);
  const [wd14Done, setWd14Done] = useState(false);
  const [e621Done, setE621Done] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);

  // Tags state — per-file and merged
  const [tagsByFile, setTagsByFile] = useState<Map<string, AiTagPrediction[]>>(new Map());
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [applying, setApplying] = useState(false);
  const [activeFileIndex, setActiveFileIndex] = useState(0);

  // Position panel
  const MARGIN = 12;
  const showSidebar = hashes.length > 1;
  const panelWidth = showSidebar ? 520 : 360;

  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const anchorRect = anchorEl.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();

    let x: number;
    let y: number;

    if (anchorPoint) {
      x = anchorPoint.x;
      y = anchorPoint.y + 4;
    } else {
      const inspectorEl = anchorEl.closest('[class*="panel"]') as HTMLElement | null;
      const inspectorLeft = inspectorEl ? inspectorEl.getBoundingClientRect().left : anchorRect.left;
      x = inspectorLeft - elRect.width - 4;
      y = anchorRect.top;
    }

    const maxX = window.innerWidth - elRect.width - MARGIN;
    const maxY = window.innerHeight - elRect.height - MARGIN;
    x = Math.max(MARGIN, Math.min(x, maxX));
    y = Math.max(MARGIN, Math.min(y, maxY));

    setPos({ x, y });
  }, [anchorEl, anchorPoint]);

  // Dragging (on footer)
  const dragStart = useRef<{ mx: number; my: number; anchor: number; y: number } | null>(null);

  const onFooterMouseDown = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('button')) return;
    const el = menuRef.current;
    const rect = el?.getBoundingClientRect();
    dragStart.current = { mx: e.clientX, my: e.clientY, anchor: rect ? rect.left : pos.x, y: pos.y };
  }, [pos.x, pos.y]);

  const draggingRef = useRef(false);
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const ds = dragStart.current;
      if (!ds) return;
      const dx = e.clientX - ds.mx, dy = e.clientY - ds.my;
      if (!draggingRef.current && Math.abs(dx) + Math.abs(dy) < 5) return;
      if (!draggingRef.current) { draggingRef.current = true; setDragging(true); }
      e.preventDefault();
      const el = menuRef.current;
      const w = el?.offsetWidth ?? 0, h = el?.offsetHeight ?? 0;
      const x = Math.max(MARGIN, Math.min(ds.anchor + dx, window.innerWidth - w - MARGIN));
      const y = Math.max(MARGIN, Math.min(ds.y + dy, window.innerHeight - h - MARGIN));
      setPos({ x, y });
    };
    const onUp = () => { dragStart.current = null; draggingRef.current = false; setDragging(false); };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    return () => { document.removeEventListener('mousemove', onMove); document.removeEventListener('mouseup', onUp); };
  }, []);

  // Run a model — process files individually for progress tracking
  const runModel = async (slug: string) => {
    const setRunning = slug === WD14_SLUG ? setWd14Running : setE621Running;
    const setDone = slug === WD14_SLUG ? setWd14Done : setE621Done;

    setRunning(true);
    const total = hashes.length;
    setProgress({ done: 0, total });

    try {
      for (let i = 0; i < hashes.length; i++) {
        setProgress({ done: i, total });
        const hash = hashes[i];
        const result = await api.aiTagger.predict([hash], [slug]);
        const newTags: AiTagPrediction[] = [];
        for (const pred of result.predictions) {
          for (const tag of pred.tags) newTags.push(tag);
        }

        // Store per-file tags (merge with existing from other model runs)
        setTagsByFile((prev) => {
          const next = new Map(prev);
          const existing = next.get(hash) ?? [];
          const merged = new Map<string, AiTagPrediction>();
          for (const tag of existing) merged.set(tagKey(tag.namespace, tag.tag), tag);
          for (const tag of newTags) {
            const key = tagKey(tag.namespace, tag.tag);
            const ex = merged.get(key);
            if (!ex || tag.confidence > ex.confidence) merged.set(key, tag);
          }
          next.set(hash, Array.from(merged.values()));
          return next;
        });

        // Select new tags by default
        setSelected((prev) => {
          const next = new Set(prev);
          for (const tag of newTags) next.add(tagKey(tag.namespace, tag.tag));
          return next;
        });
      }

      setDone(true);
    } catch (err) {
      console.error(`AI tagger ${slug} failed:`, err);
    } finally {
      setRunning(false);
      setProgress(null);
    }
  };

  // Group tags by namespace
  // Merge all files' tags for the "All" view, or show active file's tags
  const allTags = useMemo(() => {
    const merged = new Map<string, AiTagPrediction>();
    for (const fileTags of tagsByFile.values()) {
      for (const tag of fileTags) {
        const key = tagKey(tag.namespace, tag.tag);
        const existing = merged.get(key);
        if (!existing || tag.confidence > existing.confidence) {
          merged.set(key, tag);
        }
      }
    }
    return Array.from(merged.values());
  }, [tagsByFile]);

  const activeHash = hashes[activeFileIndex] ?? null;
  const displayTags = activeHash && hashes.length > 1
    ? (tagsByFile.get(activeHash) ?? [])
    : allTags;

  const grouped = useMemo(() => {
    const map = new Map<string, AiTagPrediction[]>();
    for (const tag of displayTags) {
      const list = map.get(tag.namespace) ?? [];
      list.push(tag);
      map.set(tag.namespace, list);
    }
    for (const list of map.values()) {
      list.sort((a, b) => b.confidence - a.confidence);
    }
    return map;
  }, [displayTags]);

  const orderedNamespaces = NAMESPACE_ORDER.filter((ns) => grouped.has(ns));

  const toggleTag = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleApply = async () => {
    setApplying(true);
    try {
      // Apply per-file: each file only gets its own predicted tags (filtered by selection)
      for (const [hash, fileTags] of tagsByFile.entries()) {
        const tagsForFile: string[] = [];
        for (const tag of fileTags) {
          if (selected.has(tagKey(tag.namespace, tag.tag))) {
            tagsForFile.push(formatTagForApply(tag.namespace, tag.tag));
          }
        }
        if (tagsForFile.length > 0) {
          await api.aiTagger.apply([hash], tagsForFile);
        }
      }
      await onApply([]); // signal completion (for refresh)
      onClose();
    } catch (err) {
      console.error('[AiTagger] Apply failed:', err);
    } finally {
      setApplying(false);
    }
  };

  return (
    <OverlayShell open onClose={onClose} pinned={pinned}>
      <div
        ref={menuRef}
        className={`${st.panel}${dragging ? ` ${st.panelDragging}` : ''}`}
        style={{ left: pos.x, top: pos.y, width: panelWidth }}
        onContextMenu={(e) => e.preventDefault()}
      >
        {/* Header — model run buttons + pin */}
        <div className={st.header}>
          <div className={st.modelButtons}>
            <TextButton
              compact
              onClick={() => void runModel(WD14_SLUG)}
              disabled={!wd14Available || wd14Done || wd14Running}
              style={{ flex: 1 }}
              title={!wd14Available ? 'Enable and download WD14 in Settings' : undefined}
            >
              {wd14Running ? <Loader size={12} /> : wd14Done ? <IconCheck size={12} /> : <IconSparkles size={12} />}
              {wd14Done ? 'WD14 Done' : 'Tag with WD14'}
            </TextButton>
            <TextButton
              compact
              onClick={() => void runModel(E621_SLUG)}
              disabled={!e621Available || e621Done || e621Running}
              style={{ flex: 1 }}
              title={!e621Available ? 'Enable and download E621 in Settings' : undefined}
            >
              {e621Running ? <Loader size={12} /> : e621Done ? <IconCheck size={12} /> : <IconSparkles size={12} />}
              {e621Done ? 'E621 Done' : 'Tag with E621'}
            </TextButton>
          </div>
          <button
            className={st.pinBtn}
            onClick={() => setPinned((p) => !p)}
            title={pinned ? 'Unpin' : 'Pin'}
          >
            {pinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
          </button>
        </div>

        {/* Body — optional sidebar + tag list */}
        <div className={st.body}>
        {showSidebar && (
          <div className={st.sidebar}>
            {hashes.map((hash, idx) => (
              <div
                key={hash}
                className={`${st.sidebarItem}${idx === activeFileIndex ? ` ${st.sidebarItemActive}` : ''}`}
                onClick={() => setActiveFileIndex(idx)}
                title={hash}
              >
                <span className={st.sidebarIndex}>{idx + 1}</span>
                <span className={st.sidebarHash}>{hash.slice(0, 8)}</span>
                {tagsByFile.has(hash) && (
                  <span className={st.sidebarBadge}>{tagsByFile.get(hash)!.length}</span>
                )}
              </div>
            ))}
          </div>
        )}
        <div className={st.content}>
          {progress && (
            <div className={st.progressWrap}>
              <div className={st.progressBar}>
                <div
                  className={st.progressFill}
                  style={{ width: `${progress.total > 0 ? ((progress.done / progress.total) * 100) : 0}%` }}
                />
              </div>
              <span className={st.progressText}>{progress.done}/{progress.total}</span>
            </div>
          )}
          {allTags.length === 0 && !wd14Running && !e621Running ? (
            <div className={st.empty}>
              Click a model above to generate tag predictions
            </div>
          ) : allTags.length === 0 && !progress ? (
            <div className={st.empty}>
              <Loader size="xs" />
            </div>
          ) : (
            orderedNamespaces.map((ns) => {
              const tags = grouped.get(ns) ?? [];
              return (
                <div key={ns}>
                  <div className={st.sectionHeader}>
                    {NAMESPACE_LABELS[ns] ?? ns}
                  </div>
                  {tags.map((tag) => {
                    const key = tagKey(tag.namespace, tag.tag);
                    return (
                      <div key={key} className={st.tagRow}>
                        <NamespaceTagChip tag={tag.tag} namespace={tag.namespace} size="sm" />
                        <span className={st.confidence}>
                          {Math.round(tag.confidence * 100)}%
                        </span>
                        <Checkbox
                          checked={selected.has(key)}
                          onChange={() => toggleTag(key)}
                          size="xs"
                          style={{ flexShrink: 0 }}
                        />
                      </div>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>
        </div>

        {/* Footer — shortcuts + apply (draggable) */}
        <div className={st.footer} onMouseDown={onFooterMouseDown}>
          <div className={st.footerLeft}>
            <span className={st.shortcutTip}><span className={st.kbd}>ESC</span> Close</span>
          </div>
          <div className={st.footerRight}>
            <TextButton compact onClick={onClose}>Cancel</TextButton>
            <TextButton
              compact
              onClick={handleApply}
              disabled={applying || selected.size === 0}
            >
              {applying ? <Loader size={10} /> : null}
              Apply{selected.size > 0 ? ` ${selected.size}` : ''}
            </TextButton>
          </div>
        </div>
      </div>
    </OverlayShell>
  );
}
