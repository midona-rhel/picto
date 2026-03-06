import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { IconDownload, IconLayoutSidebar, IconFolder, IconFolderQuestion, IconFolderStar, IconPhoto, IconInbox, IconTag, IconTrash, IconClock, IconCopy } from '@tabler/icons-react';
import { api, getCurrentWindow } from '#desktop/api';
import { useNavigationStore } from './stores/navigationStore';
import { useSettingsStore, type AppSettings } from './stores/settingsStore';
import { useDomainStore } from './stores/domainStore';
import { CommandPalette, type CommandAction } from '#features/app/components';
import { SHORTCUT_DEFS, formatKeysDisplay, getShortcut, matchesShortcutDef } from './lib/shortcuts';
import { GridViewMode, ImageGridControls, FilterBar, ImagePropertiesPanel, DragGhost } from '#features/grid/components';
import { MainViewModelProvider, MainViewRouter, CreateFlowModal, WindowControls } from '#features/layout/components';
import { Sidebar, SidebarMenuButton } from '#features/sidebar/components';
import { TagPickerPortal } from './services/TagPickerPortal';
import { TagSelectPortal } from '#features/tags/components';
import { FolderPickerPortal } from './services/FolderPickerPortal';
import { KbdTooltip } from './components/ui/KbdTooltip';
import { useScopedGridPreferences } from './hooks/useScopedGridPreferences';
import { ScopedDisplayProvider } from './contexts/ScopedDisplayContext';
import { useAppBootstrap } from './app-shell/useAppBootstrap';
import { useInspectorState } from './features/inspector/hooks/useInspectorState';
import { useGridFeatureState } from './hooks/useGridFeatureState';
import styles from './App.module.css';

const isMac = navigator.platform.includes('Mac');

/** Parse a shortcut key string (e.g. "Mod+Shift+T") into KeyboardEvent init values. */
function parseShortcutKeys(keys: string): { key: string; code: string; meta: boolean; ctrl: boolean; alt: boolean; shift: boolean } | null {
  const parts = keys.split('+');
  let key = '';
  let meta = false;
  let ctrl = false;
  let alt = false;
  let shift = false;
  for (const p of parts) {
    const lower = p.toLowerCase();
    if (lower === 'mod') { if (isMac) meta = true; else ctrl = true; }
    else if (lower === 'ctrl') ctrl = true;
    else if (lower === 'alt') alt = true;
    else if (lower === 'shift') shift = true;
    else key = p;
  }
  if (!key) return null;
  // Normalize key name to what KeyboardEvent expects
  const keyMap: Record<string, string> = {
    'Backspace': 'Backspace', 'Delete': 'Delete', 'Enter': 'Enter', 'Escape': 'Escape',
    'ArrowLeft': 'ArrowLeft', 'ArrowRight': 'ArrowRight', 'ArrowUp': 'ArrowUp', 'ArrowDown': 'ArrowDown',
    'Tab': 'Tab', 'Space': ' ', 'F2': 'F2',
  };
  const resolvedKey = keyMap[key] ?? key.toLowerCase();
  const code = resolvedKey.length === 1 ? `Key${resolvedKey.toUpperCase()}` : resolvedKey;
  return { key: resolvedKey, code, meta, ctrl, alt, shift };
}

function App() {
  const startupTsRef = useRef<number>(performance.now());
  const [shellVisible, setShellVisible] = useState(false);
  const [flowRefreshToken, setFlowRefreshToken] = useState(0);

  // --- Navigation ---
  const {
    currentView, activeSmartFolder, activeFolder, activeCollection, activeFlow, activeStatusFilter, filterTags,
    canGoBack, canGoForward,
    goBack, goForward,
    setActiveSmartFolder,
  } = useNavigationStore();

  // --- Settings ---
  const { settings, updateSetting, loaded: settingsLoaded } = useSettingsStore();

  // --- Sidebar data ---
  const { allImagesCount, inboxCount, uncategorizedCount, trashCount, smartFolderCounts, folderNodes } = useDomainStore();

  // --- Bootstrap (init, theme, events, menu, hotkeys, titlebar drag) ---
  const { handleTitlebarMouseDown, displayedTitle, handleScopeTransitionMidpoint } =
    useAppBootstrap();

  // --- Inspector state ---
  const inspector = useInspectorState({
    showInspectorSetting: settings.showInspector,
    currentView,
    propertiesPanelWidth: settings.propertiesPanelWidth,
  });

  // --- Grid feature state (search, filters, flows, folder sort) ---
  const grid = useGridFeatureState({
    currentView,
    isDetailMode: inspector.isDetailMode,
    activeFolder,
    activeSmartFolder,
    setActiveSmartFolder,
    filterTags,
    allImagesCount,
    activeStatusFilter,
    inboxCount,
    uncategorizedCount,
    trashCount,
    smartFolderCounts,
    folderNodes,
    selectedImages: inspector.selectedImages,
  });

  // --- Scoped grid view preferences ---
  const defaultGridViewMode = settings.gridViewMode as GridViewMode;
  const [gridContainerWidth, setGridContainerWidth] = useState(0);
  const defaultDisplayOptions = useMemo(() => ({
    showTileName: settings.showTileName,
    showResolution: settings.showResolution,
    showExtension: settings.showExtension,
    showExtensionLabel: settings.showExtensionLabel,
    thumbnailFitMode: (settings.thumbnailFitMode ?? 'contain') as 'contain' | 'cover',
  }), [settings.showTileName, settings.showResolution, settings.showExtension, settings.showExtensionLabel, settings.thumbnailFitMode]);
  const {
    gridViewMode, gridTargetSize, gridSortField, gridSortOrder,
    displayOptions,
    handleGridViewModeChange, handleGridTargetSizeChange,
    handleGridSortFieldChange, handleGridSortOrderChange,
    handleDisplayOptionChange,
  } = useScopedGridPreferences({
    currentView,
    activeFolderId: activeFolder?.folder_id ?? null,
    activeSmartFolderId: activeSmartFolder?.id ?? null,
    activeStatusFilter,
    settingsLoaded,
    defaultGridViewMode,
    defaultGridTargetSize: settings.gridTargetSize,
    defaultSortField: settings.gridSortField,
    defaultSortOrder: settings.gridSortOrder,
    defaultDisplayOptions,
  });

  const scopedDisplayValue = useMemo(
    () => ({ displayOptions, onDisplayOptionChange: handleDisplayOptionChange }),
    [displayOptions, handleDisplayOptionChange],
  );

  const showSidebar = settings.showSidebar;
  const isImagesView = currentView === 'images';
  const panelsVisible = showSidebar || settings.showInspector;
  const togglePanels = useCallback(() => {
    if (panelsVisible) {
      updateSetting('showSidebar', false);
      updateSetting('showInspector', false);
    } else {
      updateSetting('showSidebar', true);
      updateSetting('showInspector', true);
    }
  }, [panelsVisible, updateSetting]);
  const openSubscriptionsWindow = useCallback(() => {
    api.os.openSubscriptionsWindow().catch(() => {});
  }, []);

  // ── Always on top ──────────────────────────────────────────────
  const [alwaysOnTop, setAlwaysOnTop] = useState(false);
  const toggleAlwaysOnTop = useCallback(() => {
    const next = !alwaysOnTop;
    setAlwaysOnTop(next);
    getCurrentWindow().setAlwaysOnTop(next).catch(() => {});
  }, [alwaysOnTop]);

  // ── Command Palette ─────────────────────────────────────────────
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteMode, setPaletteMode] = useState<'all' | 'navigation'>('all');
  const { smartFolders } = useDomainStore();
  const { navigateToFolder, navigateToSmartFolder, navigateTo } = useNavigationStore();

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const cmdPalette = getShortcut('nav.commandPalette');
      if (cmdPalette && matchesShortcutDef(e, cmdPalette)) {
        e.preventDefault();
        setPaletteMode('all');
        setPaletteOpen(true);
        return;
      }
      const goToFolder = getShortcut('nav.goToFolder');
      if (goToFolder && matchesShortcutDef(e, goToFolder)) {
        e.preventDefault();
        setPaletteMode('navigation');
        setPaletteOpen(true);
        return;
      }
      const back = getShortcut('nav.back');
      if (back && matchesShortcutDef(e, back)) {
        e.preventDefault();
        if (canGoBack) goBack();
        return;
      }
      const forward = getShortcut('nav.forward');
      if (forward && matchesShortcutDef(e, forward)) {
        e.preventDefault();
        if (canGoForward) goForward();
        return;
      }
      const aot = getShortcut('view.alwaysOnTop');
      if (aot && matchesShortcutDef(e, aot)) {
        e.preventDefault();
        toggleAlwaysOnTop();
        return;
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [canGoBack, canGoForward, goBack, goForward, toggleAlwaysOnTop]);

  const paletteActions = useMemo((): CommandAction[] => {
    const actions: CommandAction[] = [];

    // System navigation targets
    const navTargets: { id: string; label: string; icon: React.ReactNode; go: () => void }[] = [
      { id: 'go.allImages', label: 'All Images', icon: <IconPhoto size={16} />, go: () => navigateTo('images', null, null, null) },
      { id: 'go.inbox', label: 'Inbox', icon: <IconInbox size={16} />, go: () => navigateTo('images', null, null, 'inbox') },
      { id: 'go.uncategorized', label: 'Uncategorized', icon: <IconFolderQuestion size={16} />, go: () => navigateTo('images', null, null, 'uncategorized') },
      { id: 'go.untagged', label: 'Untagged', icon: <IconTag size={16} />, go: () => navigateTo('images', null, null, 'untagged') },
      { id: 'go.trash', label: 'Trash', icon: <IconTrash size={16} />, go: () => navigateTo('images', null, null, 'trash') },
      { id: 'go.recentViewed', label: 'Recently Viewed', icon: <IconClock size={16} />, go: () => navigateTo('images', null, null, 'recently_viewed') },
      { id: 'go.duplicates', label: 'Duplicates', icon: <IconCopy size={16} />, go: () => navigateTo('duplicates') },
    ];
    for (const t of navTargets) {
      actions.push({ id: t.id, label: t.label, group: 'Navigation', icon: t.icon, execute: t.go });
    }

    // Dynamic folders
    for (const node of folderNodes) {
      if (node.kind === 'folder') {
        const folderId = parseInt(node.id.replace('folder:', ''), 10);
        if (isNaN(folderId)) continue;
        actions.push({
          id: `go.folder.${folderId}`,
          label: node.name,
          group: 'Navigation',
          icon: <IconFolder size={16} />,
          execute: () => navigateToFolder({ folder_id: folderId, name: node.name }),
        });
      }
    }

    // Dynamic smart folders
    for (const sf of smartFolders) {
      actions.push({
        id: `go.sf.${sf.id}`,
        label: sf.name,
        group: 'Navigation',
        icon: <IconFolderStar size={16} />,
        execute: () => navigateToSmartFolder({ id: sf.id, name: sf.name, predicate: sf.predicate as any }),
      });
    }

    // Shortcut-based actions (skip nav ones we already added, and skip palette itself)
    const skipIds = new Set(['nav.commandPalette', 'nav.goToFolder', 'nav.allImages', 'nav.inbox', 'nav.untagged', 'nav.trash', 'nav.recentViewed']);
    for (const def of SHORTCUT_DEFS) {
      if (skipIds.has(def.id)) continue;
      actions.push({
        id: `shortcut.${def.id}`,
        label: def.label,
        description: def.description,
        group: def.group,
        shortcut: formatKeysDisplay(def.keys),
        execute: () => {
          // Dispatch a synthetic keyboard event to trigger the existing handler
          const parsed = parseShortcutKeys(def.keys);
          if (parsed) {
            window.dispatchEvent(new KeyboardEvent('keydown', {
              key: parsed.key,
              code: parsed.code,
              metaKey: parsed.meta,
              ctrlKey: parsed.ctrl,
              altKey: parsed.alt,
              shiftKey: parsed.shift,
              bubbles: true,
            }));
          }
        },
      });
    }

    return actions;
  }, [folderNodes, smartFolders, navigateTo, navigateToFolder, navigateToSmartFolder]);

  const [displayControlsFolderId, setDisplayControlsFolderId] = useState<number | null>(
    activeFolder?.folder_id ?? null,
  );

  // Hide startup churn (title/filter/control relayout + first grid pass) behind a short reveal.
  useEffect(() => {
    if (!settingsLoaded) return;
    const elapsed = performance.now() - startupTsRef.current;
    const remainingMs = Math.max(0, 500 - elapsed);
    const timer = setTimeout(() => setShellVisible(true), remainingMs);
    return () => clearTimeout(timer);
  }, [settingsLoaded]);

  // Outside images transitions, keep controls scope in sync immediately.
  useEffect(() => {
    if (currentView !== 'images') {
      setDisplayControlsFolderId(activeFolder?.folder_id ?? null);
    }
  }, [currentView, activeFolder?.folder_id]);

  const handleGridScopeTransitionMidpoint = useCallback(() => {
    handleScopeTransitionMidpoint();
    const nav = useNavigationStore.getState();
    setDisplayControlsFolderId(nav.activeFolder?.folder_id ?? null);
  }, [handleScopeTransitionMidpoint]);

  const mainViewModel = useMemo(
    () => ({
      navigation: {
        currentView,
        activeSmartFolderPredicate: activeSmartFolder?.predicate,
        activeSmartFolderSortField: activeSmartFolder?.sort_field ?? undefined,
        activeSmartFolderSortOrder: activeSmartFolder?.sort_order ?? undefined,
        activeFolderId: activeFolder?.folder_id ?? null,
        activeCollectionId: activeCollection?.id ?? null,
        activeStatusFilter,
      },
      grid: {
        viewMode: gridViewMode,
        targetSize: gridTargetSize,
        sortField: gridSortField,
        sortOrder: gridSortOrder,
        searchTags: grid.effectiveSearchTags,
        excludedSearchTags: grid.excludedSearchTags,
        tagMatchMode: grid.tagMatchMode,
        searchText: grid.searchText,
        filterSearchText: grid.filterSearchText,
        filterFolderIds: grid.filterFolderIds,
        excludedFilterFolderIds: grid.excludedFilterFolderIds,
        folderMatchMode: grid.folderMatchMode,
        ratingFilter: grid.ratingFilter,
        mimePrefixes: grid.mimePrefixes,
        colorHex: grid.debouncedColorHex,
        colorAccuracy: grid.debouncedColorAccuracy,
        filterRefreshTrigger: grid.smartFolderRefresh,
        selectedScopeCount: grid.activeGridScopeCount,
      },
      gridActions: {
        onContainerWidthChange: setGridContainerWidth,
        onViewModeChange: handleGridViewModeChange,
        onSortFieldChange: (v: string) => handleGridSortFieldChange(v as AppSettings['gridSortField']),
        onSortOrderChange: (v: string) => handleGridSortOrderChange(v as AppSettings['gridSortOrder']),
        onScopeTransitionMidpoint: handleGridScopeTransitionMidpoint,
      },
      selection: {
        onSelectedImagesChange: inspector.handleSelectedImagesChange,
        onSelectionSummarySpecChange: inspector.setSelectionSummarySpec,
        onDetailViewStateChange: inspector.handleDetailViewStateChange,
      },
      flows: {
        activeFlowId: activeFlow?.id,
        flowLastResults: grid.flowLastResults,
        setFlowLastResults: grid.setFlowLastResults,
        flowRefreshToken,
        onOpenCreateFlowModal: () => grid.setCreateFlowModalOpen(true),
      },
    }),
    [
      currentView,
      activeSmartFolder?.predicate,
      activeSmartFolder?.sort_field,
      activeSmartFolder?.sort_order,
      activeFolder?.folder_id,
      activeCollection?.id,
      activeStatusFilter,
      gridViewMode,
      gridTargetSize,
      gridSortField,
      gridSortOrder,
      grid.effectiveSearchTags,
      grid.excludedSearchTags,
      grid.tagMatchMode,
      grid.searchText,
      grid.filterSearchText,
      grid.filterFolderIds,
      grid.excludedFilterFolderIds,
      grid.folderMatchMode,
      grid.ratingFilter,
      grid.mimePrefixes,
      grid.debouncedColorHex,
      grid.debouncedColorAccuracy,
      grid.smartFolderRefresh,
      grid.activeGridScopeCount,
      handleGridViewModeChange,
      handleGridSortFieldChange,
      handleGridSortOrderChange,
      handleGridScopeTransitionMidpoint,
      inspector.handleSelectedImagesChange,
      inspector.setSelectionSummarySpec,
      inspector.handleDetailViewStateChange,
      activeFlow?.id,
      grid.flowLastResults,
      grid.setFlowLastResults,
      flowRefreshToken,
      grid.setCreateFlowModalOpen,
    ],
  );

  return (
    <div
      className={`${styles.root} ${shellVisible ? styles.shellVisible : styles.shellHidden}`}
    >
      {/* Titlebar */}
      <div
        onMouseDown={handleTitlebarMouseDown}
        className={styles.titlebar}
        style={{
          right: 'var(--inspector-width, 0px)',
          gridTemplateColumns: showSidebar ? `var(--sidebar-width) 1fr` : 'auto 1fr',
        }}
      >
        <div className={styles.titlebarBurgerAnchor}>
          <SidebarMenuButton />
        </div>
        <div className={showSidebar ? (isMac ? styles.titlebarLeft : styles.titlebarLeftDesktop) : (isMac ? styles.titlebarLeftMin : styles.titlebarLeftMinDesktop)}>
          <div style={{ flex: 1 }} />
          <div className={styles.titlebarLeftActions}>
            <KbdTooltip label="Subscriptions" shortcut="Mod+Shift+S">
              <button className={`${styles.panelToggleBtn} no-drag-region`} onClick={openSubscriptionsWindow}>
                <IconDownload size={16} />
              </button>
            </KbdTooltip>
            <KbdTooltip label={panelsVisible ? 'Hide panels' : 'Show panels'} shortcut="Tab">
              <button className={`${styles.panelToggleBtn} no-drag-region`} onClick={togglePanels}>
                <IconLayoutSidebar size={16} />
              </button>
            </KbdTooltip>
          </div>
        </div>
        <div className={styles.titlebarRight}>
          <div className={styles.titlebarControls}>
            <ImageGridControls
              title={displayedTitle}
              onBack={goBack}
              onForward={goForward}
              canGoBack={canGoBack}
              canGoForward={canGoForward}
              showSizeControls={isImagesView}
              showSearch={isImagesView}
              targetSize={gridTargetSize}
              onTargetSizeChange={handleGridTargetSizeChange}
              containerWidth={gridContainerWidth}
              sortField={gridSortField}
              sortOrder={gridSortOrder}
              onSortFieldChange={(v) => handleGridSortFieldChange(v as AppSettings['gridSortField'])}
              onSortOrderChange={(v) => handleGridSortOrderChange(v as AppSettings['gridSortOrder'])}
              folderId={displayControlsFolderId}
              onSortFolderAction={grid.handleSortFolderAction}
              onReverseFolderAction={grid.handleReverseFolderAction}
              onReverseSelectedAction={grid.handleReverseSelectedAction}
              viewMode={gridViewMode}
              onViewModeChange={handleGridViewModeChange}
              searchText={grid.searchText}
              onSearchTextChange={grid.setSearchText}
              detailViewState={inspector.detailViewState}
              detailViewControls={inspector.detailViewControls}
            />
          </div>
          {!isMac && !inspector.showInspector && <WindowControls />}
        </div>
      </div>

      {/* Filter bar */}
      {isImagesView && (
        <FilterBar
          visible={grid.showFilterBar}
          showSidebar={showSidebar}
          showInspector={inspector.showInspector}
          searchTags={grid.searchTags}
          excludedSearchTags={grid.excludedSearchTags}
          tagLogicMode={grid.tagLogicMode}
          onSearchTagsChange={grid.setSearchTags}
          onExcludedSearchTagsChange={grid.setExcludedSearchTags}
          onTagLogicModeChange={grid.setTagLogicMode}
        />
      )}

      {/* Main layout */}
      <div className={styles.layout}>
        {showSidebar && (
          <div className={styles.sidebar}>
            <Sidebar onSmartFolderUpdated={grid.handleSmartFolderUpdated} />
          </div>
        )}

        <div className={styles.mainContent}>
          <ScopedDisplayProvider value={scopedDisplayValue}>
            <MainViewModelProvider value={mainViewModel}>
              <MainViewRouter />
            </MainViewModelProvider>
          </ScopedDisplayProvider>
        </div>

        {inspector.showInspector && (
          <ImagePropertiesPanel
            selectedImages={inspector.selectedImages}
            selectionSummarySpec={inspector.selectionSummarySpec}
            imageName={inspector.imageName}
            onImageNameChange={inspector.handleNameChange}
            width={settings.propertiesPanelWidth}
            onWidthChange={(w) => updateSetting('propertiesPanelWidth', w)}
            onResizeDragChange={inspector.setInspectorResizeDragging}
            titlebarHeight={48}
            onTitlebarMouseDown={handleTitlebarMouseDown}
            isPinned={inspector.isPinned}
            onTogglePin={inspector.togglePin}
            fileTags={inspector.fileTags}
            fileMetadata={inspector.fileMetadata}
            collectionSummary={inspector.collectionSummary}
            selectionSummary={inspector.selectionSummary}
            fileFolders={inspector.fileFolders}
            sourceUrls={inspector.sourceUrls}
            notes={inspector.notes}
            onAddTags={inspector.onAddTags}
            onRemoveTags={inspector.onRemoveTags}
            onUpdateRating={inspector.onUpdateRating}
            onUpdateSourceUrls={inspector.onUpdateSourceUrls}
            onUpdateNotes={inspector.onUpdateNotes}
            onAddToFolders={inspector.onAddToFolders}
            onRemoveFromFolder={inspector.onRemoveFromFolder}
            onReanalyzeColors={inspector.onReanalyzeColors}
          />
        )}
      </div>

      <TagPickerPortal />
      <TagSelectPortal />
      <FolderPickerPortal />
      <DragGhost />
      <CreateFlowModal
        opened={grid.createFlowModalOpen}
        onClose={() => grid.setCreateFlowModalOpen(false)}
        onCreated={() => setFlowRefreshToken((v) => v + 1)}
      />
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} mode={paletteMode} actions={paletteActions} />
    </div>
  );
}

export default App;
