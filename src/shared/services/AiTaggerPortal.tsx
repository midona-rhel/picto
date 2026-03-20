import { useState, useEffect, useCallback, useRef, useLayoutEffect, useMemo } from 'react';
import { Checkbox, Loader } from '@mantine/core';
import { IconPin, IconPinFilled, IconSparkles, IconCheck } from '@tabler/icons-react';
import { TextButton } from '../components/TextButton';
import { api } from '#desktop/api';
import { OverlayShell } from '../components/OverlayShell';
import { NamespaceTagChip } from '../components/NamespaceTagChip';
import { registerAiTaggerOpenHandler, type AiTaggerRequest } from './aiTaggerService';
import type { AiTagPrediction } from '../types/api';
import st from './AiTaggerPortal.module.css';

const WD14_SLUG = 'wd14-swinv2-v3';
const E621_SLUG = 'z3d-e621-convnext';

const NAMESPACE_ORDER = ['general', 'character', 'copyright', 'artist', 'species', 'rating'];
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
      hashes={request.hashes}
      onApply={request.onApply}
      onClose={handleClose}
    />
  );
}

function AiTaggerPanel({
  anchorEl,
  hashes,
  onApply,
  onClose,
}: {
  anchorEl: HTMLElement;
  hashes: string[];
  onApply: (tags: string[]) => Promise<void>;
  onClose: () => void;
}) {
  const [pinned, setPinned] = useState(false);
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  // Model run state
  const [wd14Running, setWd14Running] = useState(false);
  const [e621Running, setE621Running] = useState(false);
  const [wd14Done, setWd14Done] = useState(false);
  const [e621Done, setE621Done] = useState(false);

  // Tags state
  const [allTags, setAllTags] = useState<AiTagPrediction[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [applying, setApplying] = useState(false);

  // Position panel
  const MARGIN = 12;
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const anchorRect = anchorEl.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();

    // Spawn to the left of the inspector panel
    const inspectorEl = anchorEl.closest('[class*="panel"]') as HTMLElement | null;
    const inspectorLeft = inspectorEl ? inspectorEl.getBoundingClientRect().left : anchorRect.left;
    let x = inspectorLeft - elRect.width - 4;
    let y = anchorRect.top;

    const maxX = window.innerWidth - elRect.width - MARGIN;
    const maxY = window.innerHeight - elRect.height - MARGIN;
    x = Math.max(MARGIN, Math.min(x, maxX));
    y = Math.max(MARGIN, Math.min(y, maxY));

    setPos({ x, y });
  }, [anchorEl]);

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

  // Run a model
  const runModel = async (slug: string) => {
    const setRunning = slug === WD14_SLUG ? setWd14Running : setE621Running;
    const setDone = slug === WD14_SLUG ? setWd14Done : setE621Done;

    setRunning(true);
    try {
      const result = await api.aiTagger.predict(hashes, [slug]);
      const newTags: AiTagPrediction[] = [];
      for (const pred of result.predictions) {
        for (const tag of pred.tags) {
          newTags.push(tag);
        }
      }

      setAllTags((prev) => {
        const merged = new Map<string, AiTagPrediction>();
        for (const tag of prev) merged.set(`${tag.namespace}:${tag.tag}`, tag);
        for (const tag of newTags) {
          const key = `${tag.namespace}:${tag.tag}`;
          const existing = merged.get(key);
          if (!existing || tag.confidence > existing.confidence) {
            merged.set(key, tag);
          }
        }
        return Array.from(merged.values());
      });

      // Select all new tags by default
      setSelected((prev) => {
        const next = new Set(prev);
        for (const tag of newTags) next.add(`${tag.namespace}:${tag.tag}`);
        return next;
      });

      setDone(true);
    } catch (err) {
      console.error(`AI tagger ${slug} failed:`, err);
    } finally {
      setRunning(false);
    }
  };

  // Group tags by namespace
  const grouped = useMemo(() => {
    const map = new Map<string, AiTagPrediction[]>();
    for (const tag of allTags) {
      const list = map.get(tag.namespace) ?? [];
      list.push(tag);
      map.set(tag.namespace, list);
    }
    for (const list of map.values()) {
      list.sort((a, b) => b.confidence - a.confidence);
    }
    return map;
  }, [allTags]);

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
      await onApply(Array.from(selected));
      onClose();
    } catch (err) {
      console.error('Failed to apply AI tags:', err);
    } finally {
      setApplying(false);
    }
  };

  return (
    <OverlayShell open onClose={onClose} pinned={pinned}>
      <div
        ref={menuRef}
        className={`${st.panel}${dragging ? ` ${st.panelDragging}` : ''}`}
        style={{ left: pos.x, top: pos.y }}
        onContextMenu={(e) => e.preventDefault()}
      >
        {/* Header — model run buttons + pin */}
        <div className={st.header}>
          <div className={st.modelButtons}>
            <TextButton
              compact
              onClick={() => void runModel(WD14_SLUG)}
              disabled={wd14Done || wd14Running}
              style={{ flex: 1 }}
            >
              {wd14Running ? <Loader size={12} /> : wd14Done ? <IconCheck size={12} /> : <IconSparkles size={12} />}
              {wd14Done ? 'WD14 Done' : 'Tag with WD14'}
            </TextButton>
            <TextButton
              compact
              onClick={() => void runModel(E621_SLUG)}
              disabled={e621Done || e621Running}
              style={{ flex: 1 }}
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

        {/* Content — tag list */}
        <div className={st.content}>
          {allTags.length === 0 && !wd14Running && !e621Running ? (
            <div className={st.empty}>
              Click a model above to generate tag predictions
            </div>
          ) : allTags.length === 0 ? (
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
                    const key = `${tag.namespace}:${tag.tag}`;
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
