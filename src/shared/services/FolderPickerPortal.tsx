import { useState, useEffect, useRef, useLayoutEffect, useCallback, useMemo } from 'react';
import { OverlayShell } from '#ui/OverlayShell';
import { IconCheck, IconChevronDown, IconChevronRight, IconEqual, IconLayersIntersect, IconLayersUnion, IconMinus, IconPin, IconPinFilled } from '@tabler/icons-react';
import { useDomainStore } from '../../state/domainStore';
import type { FilterLogicMode } from '../../state/filterStore';
import { DynamicIcon } from '#features/smart-folders/components';
import { buildFolderTree, parseFolderId, type TreeNode } from '#features/sidebar/lib/folderTreeData';
import { registerFolderPickerOpenHandler, type FolderPickerRequest } from './folderPickerService';
import st from './FolderPicker.module.css';

type LogicMode = FilterLogicMode;
type FilterTab = 'all' | 'selected';

/** Flatten a tree into a list respecting expand/collapse state. */
function flattenTree(
  roots: TreeNode[],
  expanded: Set<string>,
  filter: (node: TreeNode) => boolean,
): TreeNode[] {
  const result: TreeNode[] = [];
  const walk = (nodes: TreeNode[]) => {
    for (const node of nodes) {
      if (!filter(node)) continue;
      result.push(node);
      if (node.children.length > 0 && expanded.has(node.id)) {
        walk(node.children);
      }
    }
  };
  walk(roots);
  return result;
}

/** Check if a node or any descendant matches the search. */
function matchesSearch(node: TreeNode, lowerSearch: string): boolean {
  if (node.name.toLowerCase().includes(lowerSearch)) return true;
  return node.children.some((child) => matchesSearch(child, lowerSearch));
}

/** Collect all node IDs that should be expanded to show search matches. */
function expandedForSearch(roots: TreeNode[], lowerSearch: string): Set<string> {
  const ids = new Set<string>();
  const walk = (nodes: TreeNode[]): boolean => {
    let anyMatch = false;
    for (const node of nodes) {
      const childMatch = walk(node.children);
      const selfMatch = node.name.toLowerCase().includes(lowerSearch);
      if (selfMatch || childMatch) {
        ids.add(node.id);
        anyMatch = true;
      }
    }
    return anyMatch;
  };
  walk(roots);
  return ids;
}

/** Check if a node or any descendant is selected. */
function hasSelectedDescendant(node: TreeNode, selected: Set<number>): boolean {
  const fid = parseFolderId(node.id);
  if (fid != null && selected.has(fid)) return true;
  return node.children.some((child) => hasSelectedDescendant(child, selected));
}

export function FolderPickerPortal() {
  const [request, setRequest] = useState<FolderPickerRequest | null>(null);
  const [openKey, setOpenKey] = useState(0);

  useEffect(() => {
    return registerFolderPickerOpenHandler((req) => {
      setOpenKey((k) => k + 1);
      setRequest(req);
    });
  }, []);

  const handleClose = useCallback(() => setRequest(null), []);

  if (!request) return null;

  return (
    <FolderPickerPanel
      key={openKey}
      anchorEl={request.anchorEl}
      anchorPoint={request.anchorPoint}
      initialSelected={request.selectedFolderIds}
      initialExcluded={request.excludedFolderIds}
      initialLogicMode={request.logicMode}
      onToggle={request.onToggle}
      onExclude={request.onExclude}
      onLogicChange={request.onLogicChange}
      onClose={handleClose}
    />
  );
}

function FolderPickerPanel({
  anchorEl,
  anchorPoint,
  initialSelected,
  initialExcluded,
  initialLogicMode,
  onToggle,
  onExclude,
  onLogicChange,
  onClose,
}: {
  anchorEl: HTMLElement;
  anchorPoint?: { x: number; y: number };
  initialSelected: number[];
  initialExcluded?: number[];
  initialLogicMode?: LogicMode;
  onToggle: (folderId: number, folderName: string, added: boolean) => void;
  onExclude?: (folderId: number, folderName: string) => void;
  onLogicChange?: (mode: LogicMode) => void;
  onClose: () => void;
}) {
  const isFilterMode = !!onExclude;
  const menuRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [search, setSearch] = useState('');
  const [selected, setSelected] = useState<Set<number>>(() => new Set(initialSelected));
  const [excluded, setExcluded] = useState<Set<number>>(() => new Set(initialExcluded ?? []));
  const [logic, setLogic] = useState<LogicMode>(initialLogicMode ?? 'OR');
  const [pos, setPos] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [focusIndex, setFocusIndex] = useState(-1);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [filterTab, setFilterTab] = useState<FilterTab>('all');

  const folderNodes = useDomainStore((s) => s.folderNodes);
  const countMap = useMemo(() => {
    const m = new Map<number, number>();
    for (const node of folderNodes) {
      const fid = parseFolderId(node.id);
      if (fid != null && node.count != null) m.set(fid, node.count);
    }
    return m;
  }, [folderNodes]);

  // Build folder tree from sidebar data
  const tree = useMemo(() => buildFolderTree(folderNodes), [folderNodes]);

  // Auto-expand all on first render so tree is visible
  useEffect(() => {
    const allIds = new Set<string>();
    const walk = (nodes: TreeNode[]) => {
      for (const node of nodes) {
        if (node.children.length > 0) allIds.add(node.id);
        walk(node.children);
      }
    };
    walk(tree);
    setExpanded(allIds);
  }, [tree]);

  // Flatten tree based on search, expand state, and filter tab
  const flatList = useMemo(() => {
    const lowerSearch = search.toLowerCase();
    const searchExpanded = lowerSearch ? expandedForSearch(tree, lowerSearch) : null;
    const effectiveExpanded = searchExpanded ?? expanded;

    const filter = (node: TreeNode): boolean => {
      if (lowerSearch && !matchesSearch(node, lowerSearch)) return false;
      if (filterTab === 'selected') {
        return hasSelectedDescendant(node, selected);
      }
      return true;
    };

    return flattenTree(tree, effectiveExpanded, filter);
  }, [tree, search, expanded, filterTab, selected]);

  const selectedCount = selected.size;

  // Position panel
  const MARGIN = 12;
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const anchorRect = anchorEl.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();

    let x: number;
    let y: number;

    if (anchorPoint) {
      // Context menu: spawn at mouse position, left-anchored
      x = anchorPoint.x;
      y = anchorPoint.y + 4;
    } else if (isFilterMode) {
      // Filter bar: spawn below the anchor element
      x = anchorRect.left;
      y = anchorRect.bottom + 4;
    } else {
      // Inspector button: spawn to the left of the inspector panel
      const inspectorEl = anchorEl.closest('[class*="panel"]') as HTMLElement | null;
      const inspectorLeft = inspectorEl ? inspectorEl.getBoundingClientRect().left : anchorRect.left;
      x = inspectorLeft - elRect.width - 4;
      y = anchorRect.top;
    }

    // Clamp to viewport with margin
    const maxX = window.innerWidth - elRect.width - MARGIN;
    const maxY = window.innerHeight - elRect.height - MARGIN;
    x = Math.max(MARGIN, Math.min(x, maxX));
    y = Math.max(MARGIN, Math.min(y, maxY));

    setPos({ x, y });
  }, [anchorEl, anchorPoint, folderNodes.length, isFilterMode]);

  useEffect(() => { searchRef.current?.focus(); }, []);

  // Dragging
  const dragStart = useRef<{ mx: number; my: number; anchor: number; y: number } | null>(null);
  const onHeaderMouseDown = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('input, button, [class*="logicTab"], [class*="segTab"]')) return;
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
      let x: number, y: number;
      x = Math.max(MARGIN, Math.min(ds.anchor + dx, window.innerWidth - w - MARGIN));
      y = Math.max(MARGIN, Math.min(ds.y + dy, window.innerHeight - h - MARGIN));
      setPos({ x, y });
    };
    const onUp = () => { dragStart.current = null; draggingRef.current = false; setDragging(false); };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    return () => { document.removeEventListener('mousemove', onMove); document.removeEventListener('mouseup', onUp); };
  }, []);

  useEffect(() => { setFocusIndex(-1); }, [search, filterTab]);

  const flatListRef = useRef(flatList);
  flatListRef.current = flatList;

  const toggleExpand = useCallback((nodeId: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId); else next.add(nodeId);
      return next;
    });
  }, []);

  const handleLeftClick = useCallback((folderId: number, folderName: string) => {
    if (isFilterMode) {
      setExcluded((prev) => {
        const next = new Set(prev);
        next.delete(folderId);
        return next;
      });
    }
    const wasSelected = selected.has(folderId);
    const added = !wasSelected;
    setSelected((prev) => {
      const next = new Set(prev);
      if (added) next.add(folderId); else next.delete(folderId);
      return next;
    });
    onToggle(folderId, folderName, added);
  }, [onToggle, isFilterMode, selected]);

  const handleRightClick = useCallback((e: React.MouseEvent, folderId: number, folderName: string) => {
    e.preventDefault();
    e.stopPropagation();
    if (!onExclude) return;
    setSelected((prev) => { const next = new Set(prev); next.delete(folderId); return next; });
    const wasExcluded = excluded.has(folderId);
    setExcluded((prev) => {
      const next = new Set(prev);
      if (wasExcluded) next.delete(folderId); else next.add(folderId);
      return next;
    });
    onExclude(folderId, folderName);
  }, [onExclude, excluded]);

  // Keyboard
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onClose(); return; }
      if (e.target === searchRef.current) {
        if (e.key === 'ArrowDown') { e.preventDefault(); e.stopPropagation(); setFocusIndex(0); searchRef.current?.blur(); }
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault(); e.stopPropagation();
        setFocusIndex((i) => Math.min(i + 1, flatListRef.current.length - 1));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault(); e.stopPropagation();
        setFocusIndex((i) => { const next = i - 1; if (next < 0) { searchRef.current?.focus(); return -1; } return next; });
      } else if (e.key === 'Enter') {
        e.preventDefault(); e.stopPropagation();
        setFocusIndex((i) => {
          if (i >= 0 && i < flatListRef.current.length) {
            const node = flatListRef.current[i];
            const fid = parseFolderId(node.id);
            if (fid != null) handleLeftClick(fid, node.name);
          }
          return i;
        });
      } else if (e.key === 'ArrowRight') {
        e.preventDefault(); e.stopPropagation();
        setFocusIndex((i) => {
          if (i >= 0 && i < flatListRef.current.length) {
            const node = flatListRef.current[i];
            if (node.children.length > 0 && !expanded.has(node.id)) toggleExpand(node.id);
          }
          return i;
        });
      } else if (e.key === 'ArrowLeft') {
        e.preventDefault(); e.stopPropagation();
        setFocusIndex((i) => {
          if (i >= 0 && i < flatListRef.current.length) {
            const node = flatListRef.current[i];
            if (node.children.length > 0 && expanded.has(node.id)) toggleExpand(node.id);
          }
          return i;
        });
      }
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [onClose, handleLeftClick, expanded, toggleExpand]);

  const contentRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (focusIndex < 0 || !contentRef.current) return;
    const items = contentRef.current.querySelectorAll('[data-folder-item]');
    items[focusIndex]?.scrollIntoView({ block: 'nearest' });
  }, [focusIndex]);

  return (
    <OverlayShell open onClose={onClose} pinned={pinned}>
      <div
        ref={menuRef}
        className={`${st.panel}${dragging ? ` ${st.panelDragging}` : ''}`}
        style={{ left: pos.x, top: pos.y }}
        onContextMenu={(e) => e.preventDefault()}
      >
        {/* Header */}
        <div
          className={`${st.header}${dragging ? ` ${st.headerDragging}` : ''}`}
          onMouseDown={onHeaderMouseDown}
        >
          <div className={st.searchWrap}>
            <input
              ref={searchRef}
              className={st.searchInput}
              type="search"
              placeholder="Search folders..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => e.stopPropagation()}
            />
          </div>
          {isFilterMode && (
            <>
              <span className={st.ruleLabel}>Rule</span>
              <div className={st.logicTabs}>
                {([
                  { mode: 'OR' as LogicMode, icon: <IconLayersUnion size={14} />, title: 'Any' },
                  { mode: 'AND' as LogicMode, icon: <IconLayersIntersect size={14} />, title: 'All' },
                  { mode: 'EQUAL' as LogicMode, icon: <IconEqual size={14} />, title: 'Equal' },
                ]).map(({ mode, icon, title }) => (
                  <div
                    key={mode}
                    className={`${st.logicTab}${logic === mode ? ` ${st.logicTabActive}` : ''}`}
                    onClick={() => { setLogic(mode); onLogicChange?.(mode); }}
                    title={title}
                  >
                    {icon}
                  </div>
                ))}
              </div>
            </>
          )}
          <div className={st.logicTabs}>
            <div
              className={`${st.segTab}${filterTab === 'all' ? ` ${st.segTabActive}` : ''}`}
              onClick={() => setFilterTab('all')}
            >
              All
            </div>
            <div
              className={`${st.segTab}${filterTab === 'selected' ? ` ${st.segTabActive}` : ''}`}
              onClick={() => setFilterTab('selected')}
            >
              Selected{selectedCount > 0 ? ` ${selectedCount}` : ''}
            </div>
          </div>
          <button
            className={st.pinBtn}
            onClick={() => setPinned((p) => !p)}
            title={pinned ? 'Unpin' : 'Pin'}
          >
            {pinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
          </button>
        </div>

        {/* Content — tree view */}
        <div ref={contentRef} className={st.content}>
          {flatList.map((node, idx) => {
            const fid = parseFolderId(node.id);
            if (fid == null) return null;
            const isChecked = selected.has(fid);
            const isExcludedItem = excluded.has(fid);
            const isFocused = idx === focusIndex;
            const count = countMap.get(fid);
            const folderColor = node.color ?? 'currentColor';
            const iconName = node.icon ?? 'IconFolder';
            const hasChildren = node.children.length > 0;
            const isExpanded = expanded.has(node.id);

            const itemClass = [
              st.checkItem,
              isChecked && st.checkItemChecked,
              isExcludedItem && st.checkItemExcluded,
              isFocused && st.checkItemFocused,
            ].filter(Boolean).join(' ');

            const checkClass = [
              st.checkIcon,
              isChecked && st.checkIconChecked,
              isExcludedItem && st.checkIconExcluded,
            ].filter(Boolean).join(' ');

            return (
              <div
                key={node.id}
                data-folder-item
                className={itemClass}
                style={{ paddingLeft: 8 + node.depth * 16 }}
                onClick={() => handleLeftClick(fid, node.name)}
                onContextMenu={(e) => handleRightClick(e, fid, node.name)}
              >
                {hasChildren ? (
                  <button
                    className={st.expandBtn}
                    onClick={(e) => { e.stopPropagation(); toggleExpand(node.id); }}
                  >
                    {isExpanded ? <IconChevronDown size={12} /> : <IconChevronRight size={12} />}
                  </button>
                ) : (
                  <span className={st.expandBtnPlaceholder} />
                )}
                <span className={checkClass}>
                  {isChecked && <span className={st.checkMark}><IconCheck size={10} strokeWidth={3} /></span>}
                  {isExcludedItem && <span className={st.checkMark}><IconMinus size={8} strokeWidth={3} /></span>}
                </span>
                <span className={st.folderIcon}>
                  <DynamicIcon name={iconName} size={16} color={folderColor} />
                </span>
                <span className={st.itemName}>
                  {search ? highlightMatch(node.name, search) : node.name}
                </span>
                {count != null && (
                  <span className={st.itemBadge}>{count.toLocaleString()}</span>
                )}
              </div>
            );
          })}
          {flatList.length === 0 && (
            <div className={st.empty}>
              {filterTab === 'selected' ? 'No folders selected' : 'No folders found'}
            </div>
          )}
        </div>

        {/* Footer (also draggable) */}
        <div className={st.footer} onMouseDown={onHeaderMouseDown}>
          <div className={st.footerLeft}>
            {isFilterMode ? (
              <>
                <span className={st.shortcutTip}>Select <span className={st.kbd}>L-click</span></span>
                <span className={st.shortcutTip}>Exclude <span className={st.kbd}>R-click</span></span>
              </>
            ) : (
              <>
                <span className={st.shortcutTip}><span className={st.kbd}>&uarr;&darr;</span></span>
                <span className={st.shortcutTip}><span className={st.kbd}>&crarr;</span> Select</span>
                <span className={st.shortcutTip}><span className={st.kbd}>&larr;&rarr;</span> Expand</span>
              </>
            )}
          </div>
          <div className={st.footerRight}>
            <span className={st.shortcutTip}><span className={st.kbd}>ESC</span></span>
          </div>
        </div>
      </div>
    </OverlayShell>
  );
}

function highlightMatch(text: string, query: string): React.ReactNode {
  if (!query) return text;
  const idx = text.toLowerCase().indexOf(query.toLowerCase());
  if (idx < 0) return text;
  return (
    <>
      {text.slice(0, idx)}
      <b className={st.matchHighlight}>{text.slice(idx, idx + query.length)}</b>
      {text.slice(idx + query.length)}
    </>
  );
}
