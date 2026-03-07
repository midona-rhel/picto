import { useState, useEffect, useRef, useLayoutEffect, useCallback, useMemo } from 'react';
import { createPortal } from 'react-dom';
import { IconCheck, IconEqual, IconLayersIntersect, IconLayersUnion, IconMinus, IconPin, IconPinFilled } from '@tabler/icons-react';
import { FolderController, type Folder } from '../../controllers/folderController';
import { useDomainStore } from '../../state/domainStore';
import type { FilterLogicMode } from '../../state/filterStore';
import { DynamicIcon } from '#features/folders/components';
import { registerFolderPickerOpenHandler, type FolderPickerRequest } from './folderPickerService';
import st from './FolderPicker.module.css';

type LogicMode = FilterLogicMode;

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
  const [folders, setFolders] = useState<Folder[]>([]);
  const [selected, setSelected] = useState<Set<number>>(() => new Set(initialSelected));
  const [excluded, setExcluded] = useState<Set<number>>(() => new Set(initialExcluded ?? []));
  const [logic, setLogic] = useState<LogicMode>(initialLogicMode ?? 'OR');
  const [pos, setPos] = useState({ x: 0, y: 0 });

  const [dragging, setDragging] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [focusIndex, setFocusIndex] = useState(-1);

  const folderNodes = useDomainStore((s) => s.folderNodes);
  const countMap = useMemo(() => {
    const m = new Map<number, number>();
    for (const node of folderNodes) {
      const fid = parseInt(node.id.replace('folder:', ''), 10);
      if (!isNaN(fid) && node.count != null) m.set(fid, node.count);
    }
    return m;
  }, [folderNodes]);

  useEffect(() => {
    FolderController.listFolders()
      .then(setFolders)
      .catch((e) => console.error('Failed to fetch folders:', e));
  }, []);

  const flatFolders = useMemo(() => {
    const lowerSearch = search.toLowerCase();
    let list = [...folders];
    if (lowerSearch) {
      list = list.filter((f) => f.name.toLowerCase().includes(lowerSearch));
    }
    list.sort((a, b) => a.name.localeCompare(b.name));
    return list;
  }, [folders, search]);

  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el) return;
    const anchorRect = anchorEl.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();
    const pointX = anchorPoint?.x ?? anchorRect.left;
    const pointY = anchorPoint?.y ?? anchorRect.top;

    if (isFilterMode) {
      let x = anchorPoint ? pointX : anchorRect.left;
      let y = anchorPoint ? pointY + 4 : anchorRect.bottom + 4;
      if (x + elRect.width > window.innerWidth - 8) x = window.innerWidth - elRect.width - 8;
      if (x < 8) x = 8;
      if (y + elRect.height > window.innerHeight - 8) y = window.innerHeight - elRect.height - 8;
      if (y < 8) y = 8;
      setPos({ x, y });
    } else {
      let x: number;
      let y: number;
      if (anchorPoint) {
        x = pointX + 6;
        y = pointY + 4;
      } else {
        const inspectorEl = anchorEl.closest('[class*="panel"]') as HTMLElement | null;
        const inspectorLeft = inspectorEl ? inspectorEl.getBoundingClientRect().left : anchorRect.left;
        x = window.innerWidth - inspectorLeft + 4;
        y = anchorRect.top;
      }
      if (x < 8) x = 8;
      if (x + elRect.width > window.innerWidth - 8) x = window.innerWidth - elRect.width - 8;
      if (y + elRect.height > window.innerHeight - 8) y = window.innerHeight - elRect.height - 8;
      if (y < 8) y = 8;
      setPos({ x, y });
    }
  }, [anchorEl, anchorPoint, folders.length, isFilterMode]);

  useEffect(() => { searchRef.current?.focus(); }, []);

  const dragStart = useRef<{ mx: number; my: number; anchor: number; y: number } | null>(null);
  const isFilterModeRef = useRef(isFilterMode);
  isFilterModeRef.current = isFilterMode;

  const onHeaderMouseDown = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('input, button, [class*="logicTab"]')) return;
    const el = menuRef.current;
    const rect = el?.getBoundingClientRect();
    const anchor = isFilterModeRef.current
      ? (rect ? rect.left : pos.x)
      : (rect ? window.innerWidth - rect.right : pos.x);
    dragStart.current = { mx: e.clientX, my: e.clientY, anchor, y: pos.y };
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
      if (isFilterModeRef.current) {
        x = Math.max(8, Math.min(ds.anchor + dx, window.innerWidth - w - 8));
      } else {
        x = Math.max(8, Math.min(ds.anchor - dx, window.innerWidth - w - 8));
      }
      y = Math.max(8, Math.min(ds.y + dy, window.innerHeight - h - 8));
      setPos({ x, y });
    };
    const onUp = () => { dragStart.current = null; draggingRef.current = false; setDragging(false); };
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    return () => { document.removeEventListener('mousemove', onMove); document.removeEventListener('mouseup', onUp); };
  }, []);

  useEffect(() => { setFocusIndex(-1); }, [search]);

  const flatFoldersRef = useRef(flatFolders);
  flatFoldersRef.current = flatFolders;

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
    // Remove from included
    setSelected((prev) => {
      const next = new Set(prev);
      next.delete(folderId);
      return next;
    });
    // Toggle excluded
    const wasExcluded = excluded.has(folderId);
    setExcluded((prev) => {
      const next = new Set(prev);
      if (wasExcluded) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
    onExclude(folderId, folderName);
  }, [onExclude, excluded]);

  // Capture phase so it fires before Mantine useHotkeys / ImageGrid handlers.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      // Escape always closes, regardless of focus
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        onClose();
        return;
      }

      // When typing in search, only intercept ArrowDown (to move into list)
      if (e.target === searchRef.current) {
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          e.stopPropagation();
          setFocusIndex(0);
          searchRef.current?.blur();
        }
        return;
      }

      // Navigation keys when focus is not in search
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        e.stopPropagation();
        setFocusIndex((i) => Math.min(i + 1, flatFoldersRef.current.length - 1));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        e.stopPropagation();
        setFocusIndex((i) => {
          const next = i - 1;
          if (next < 0) { searchRef.current?.focus(); return -1; }
          return next;
        });
      } else if (e.key === 'Enter') {
        e.preventDefault();
        e.stopPropagation();
        setFocusIndex((i) => {
          if (i >= 0 && i < flatFoldersRef.current.length) {
            const f = flatFoldersRef.current[i];
            handleLeftClick(f.folder_id, f.name);
          }
          return i;
        });
      }
    };
    document.addEventListener('keydown', onKeyDown, true);
    return () => document.removeEventListener('keydown', onKeyDown, true);
  }, [onClose, handleLeftClick]);

  const contentRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (focusIndex < 0 || !contentRef.current) return;
    const items = contentRef.current.querySelectorAll('[data-folder-item]');
    items[focusIndex]?.scrollIntoView({ block: 'nearest' });
  }, [focusIndex]);

  const panelClass = st.panel;

  return createPortal(
    <>
      {!pinned && (
        <div
          className={st.backdrop}
          onClick={onClose}
          onContextMenu={(e) => { e.preventDefault(); onClose(); }}
        />
      )}
      <div
        ref={menuRef}
        className={`${panelClass}${dragging ? ` ${st.panelDragging}` : ''}`}
        style={isFilterMode ? { left: pos.x, top: pos.y } : { right: pos.x, top: pos.y }}
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
              placeholder="Search..."
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
                    onClick={() => {
                      setLogic(mode);
                      onLogicChange?.(mode);
                    }}
                    title={title}
                  >
                    {icon}
                  </div>
                ))}
              </div>
            </>
          )}
          <button
            className={st.pinBtn}
            onClick={() => setPinned((p) => !p)}
            title={pinned ? 'Unpin' : 'Pin'}
          >
            {pinned ? <IconPinFilled size={14} /> : <IconPin size={14} />}
          </button>
        </div>

        {/* Content */}
        <div ref={contentRef} className={st.content}>
          {flatFolders.map((folder, idx) => {
            const isChecked = selected.has(folder.folder_id);
            const isExcludedItem = excluded.has(folder.folder_id);
            const isFocused = idx === focusIndex;
            const count = countMap.get(folder.folder_id);
            const folderColor = folder.color ?? 'currentColor';
            const iconName = folder.icon ?? 'IconFolder';

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
                key={folder.folder_id}
                data-folder-item
                className={itemClass}
                onClick={() => handleLeftClick(folder.folder_id, folder.name)}
                onContextMenu={(e) => handleRightClick(e, folder.folder_id, folder.name)}
              >
                <span className={checkClass}>
                  {isChecked && (
                    <span className={st.checkMark}>
                      <IconCheck size={10} strokeWidth={3} />
                    </span>
                  )}
                  {isExcludedItem && (
                    <span className={st.checkMark}>
                      <IconMinus size={8} strokeWidth={3} />
                    </span>
                  )}
                </span>
                <span className={st.folderIcon}>
                  <DynamicIcon name={iconName} size={16} color={folderColor} />
                </span>
                <span className={st.itemName}>
                  {search ? highlightMatch(folder.name, search) : folder.name}
                </span>
                {count != null && (
                  <span className={st.itemBadge}>{count.toLocaleString()}</span>
                )}
              </div>
            );
          })}
          {flatFolders.length === 0 && (
            <div className={st.empty}>No folders found</div>
          )}
        </div>

        {/* Footer */}
        <div className={st.footer}>
          <div className={st.footerLeft}>
            {isFilterMode ? (
              <>
                <span className={st.shortcutTip}>
                  Select <span className={st.kbd}>L-click</span>
                </span>
                <span className={st.shortcutTip}>
                  Exclude <span className={st.kbd}>R-click</span>
                </span>
              </>
            ) : (
              <>
                <span className={st.shortcutTip}><span className={st.kbd}>&uarr;&darr;</span></span>
                <span className={st.shortcutTip}><span className={st.kbd}>&crarr;</span> Select</span>
              </>
            )}
          </div>
          <div className={st.footerRight}>
            <span className={st.shortcutTip}>
              <span className={st.kbd}>ESC</span>
            </span>
          </div>
        </div>
      </div>
    </>,
    document.body,
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
