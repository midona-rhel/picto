import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { IconDownload, IconLayoutSidebar, IconSettings } from '@tabler/icons-react';
import { useHotkeys } from '@mantine/hooks';
import { useNavigationStore } from '../state-legacy/navigationStore';
import { useSettingsStore, type AppSettings } from '../state-legacy/settingsStore';
import { useExportActionStore } from '../state-legacy/exportActionStore';
import { useDomainStore } from '../state-legacy/domainStore';
import { CommandPalette, LogPanel } from '#features/app/components';
import { GridViewMode, ImageGridControls, FilterBar, InspectorPanel, DragGhost } from '#features/grid/components';
import { MainViewModelProvider, MainViewRouter, CreateSubscriptionGroupModal, WindowControls } from '#features/layout/components';
import { Sidebar, SidebarMenuButton } from '#features/sidebar/components';
import { ViewerHost } from '#features/viewer/components';
import { useViewerHost } from '#features/viewer/hooks/useViewerHost';
import { TagSelectPortal } from '#features/tags/components';
import { FolderPickerPortal } from '../shared/services/FolderPickerPortal';
import { AiTaggerPortal } from '../shared/services/AiTaggerPortal';
import { FolderWatchDialog } from '#features/folders/components';
import { KbdTooltip } from '#ui/KbdTooltip';
import { useScopedGridPreferences } from '../shared/hooks/useScopedGridPreferences';
import { ScopedDisplayProvider } from '../shared/contexts/ScopedDisplayContext';
import { useAppBootstrap } from './useAppBootstrap';
import { useCommandPalette } from './useCommandPalette';
import { useInspectorState } from '../features/inspector/hooks/useInspectorState';
import { useGridFeatureState } from '../features/grid/hooks/useGridFeatureState';
import { windowController } from '../controllers/windowController';
import { UpdateBanner } from '#ui/UpdateBanner';
import styles from './App.module.css';

const isMac = navigator.platform.includes('Mac');

function App() {
  const startupTsRef = useRef<number>(performance.now());
  const [shellVisible, setShellVisible] = useState(false);
  const [subscriptionRefreshToken, setSubscriptionRefreshToken] = useState(0);

  // --- Navigation ---
  const {
    currentView, activeSmartFolderId, activeFolderId, activeCollectionId, activeStatusFilter, filterTags,
    similarHashes,
    canGoBack, canGoForward,
    goBack, goForward,
  } = useNavigationStore();

  // --- Settings ---
  const { settings, updateSetting, loaded: settingsLoaded } = useSettingsStore();

  // --- Sidebar data ---
  const {
    allActiveCount,
    inboxCount,
    uncategorizedCount,
    trashCount,
    untaggedCount,
    smartFolders,
    smartFolderCounts,
    folderNodes,
  } = useDomainStore();

  const activeSmartFolder = useMemo(() => {
    if (!activeSmartFolderId) return null;
    const active = smartFolders.find((sf) => sf.id === activeSmartFolderId);
    if (!active) return null;
    return {
      id: active.id,
      name: active.name,
      parent_id: active.parent_id ? parseInt(active.parent_id, 10) : null,
      icon: active.icon ?? null,
      color: active.color ?? null,
      predicate: active.predicate ?? active.localPredicate ?? { groups: [] },
      sort_field: active.sort_field ?? null,
      sort_order: active.sort_order ?? null,
    };
  }, [activeSmartFolderId, smartFolders]);

  // --- Bootstrap (init, theme, events, menu, hotkeys, titlebar drag) ---
  const { handleTitlebarMouseDown, displayedTitle, handleScopeTransitionMidpoint } =
    useAppBootstrap();

  // --- Inspector state ---
  const inspector = useInspectorState({
    showInspectorSetting: settings.showInspector,
    currentView,
    inspectorWidthSetting: settings.inspectorWidth,
  });
  const viewer = useViewerHost();

  // --- Window horizontal resize tracking (freezes grid layout) ---
  const [windowHResizing, setWindowHResizing] = useState(false);
  const windowWidthAnchorRef = useRef(typeof window !== 'undefined' ? window.innerWidth : 0);
  const windowResizingRef = useRef(false);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const onResize = () => {
      const newWidth = window.innerWidth;
      if (!windowResizingRef.current) {
        // Not yet resizing — check if width moved enough to start freezing
        if (Math.abs(newWidth - windowWidthAnchorRef.current) > 2) {
          windowResizingRef.current = true;
          setWindowHResizing(true);
        }
      }
      if (windowResizingRef.current) {
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => {
          windowResizingRef.current = false;
          windowWidthAnchorRef.current = window.innerWidth;
          setWindowHResizing(false);
        }, 200);
      }
    };
    window.addEventListener('resize', onResize);
    return () => {
      window.removeEventListener('resize', onResize);
      if (timer) clearTimeout(timer);
    };
  }, []);

  // --- Grid feature state (search, filters, subscriptions, folder sort) ---
  const grid = useGridFeatureState({
    currentView,
    isDetailMode: inspector.isDetailMode,
    activeFolderId,
    activeCollectionId,
    activeSmartFolder,
    filterTags,
    allImagesCount: allActiveCount,
    activeStatusFilter,
    inboxCount,
    uncategorizedCount,
    untaggedCount,
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
    activeFolderId,
    activeCollectionId,
    activeSmartFolderId,
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
  const handleOpenSubscriptions = useCallback(() => {
    windowController.openSubscriptions().catch(() => {});
  }, []);

  const handleOpenSettings = useCallback(() => {
    windowController.openSettings().catch(() => {});
  }, []);

  // ── Command Palette ──────────
  const { paletteOpen, closePalette, paletteMode, paletteActions } = useCommandPalette();

  // ── Log Panel ──────────
  const [logPanelOpen, setLogPanelOpen] = useState(false);
  const toggleLogPanel = useCallback(() => setLogPanelOpen((v) => !v), []);
  useHotkeys([['mod+L', toggleLogPanel]]);

  const [displayControlsFolderId, setDisplayControlsFolderId] = useState<number | null>(
    activeFolderId,
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
      setDisplayControlsFolderId(activeFolderId);
    }
  }, [currentView, activeFolderId]);

  // Close media view when navigating away from images view
  useEffect(() => {
    if (currentView !== 'images' && viewer.mode) {
      viewer.close();
    }
  }, [currentView, viewer]);

  const handleGridScopeTransitionMidpoint = useCallback(() => {
    handleScopeTransitionMidpoint();
    const nav = useNavigationStore.getState();
    setDisplayControlsFolderId(nav.activeFolderId);
  }, [handleScopeTransitionMidpoint]);

  const mainViewModel = useMemo(
    () => ({
      navigation: {
        currentView,
        activeSmartFolderPredicate: activeSmartFolder?.predicate,
        activeSmartFolderSortField: activeSmartFolder?.sort_field ?? undefined,
        activeSmartFolderSortOrder: activeSmartFolder?.sort_order ?? undefined,
        activeFolderId,
        activeCollectionId,
        activeStatusFilter,
        similarHashes,
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
        collectionsOnly: grid.collectionsOnly,
        colorHex: grid.debouncedColorHex,
        colorAccuracy: grid.debouncedColorAccuracy,
        filterRefreshTrigger: grid.smartFolderRefresh,
        externalFreeze: inspector.inspectorResizeDragging || windowHResizing,
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
        onMediaViewStateChange: inspector.handleMediaViewStateChange,
      },
      subscriptions: {
        subscriptionRefreshToken,
        onOpenCreateSubscriptionGroupModal: () => grid.setCreateSubscriptionGroupModalOpen(true),
      },
      viewer,
    }),
    [
      currentView,
      activeSmartFolderId,
      activeSmartFolder?.predicate,
      activeSmartFolder?.sort_field,
      activeSmartFolder?.sort_order,
      activeFolderId,
      activeCollectionId,
      activeStatusFilter,
      similarHashes,
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
      grid.collectionsOnly,
      grid.debouncedColorHex,
      grid.debouncedColorAccuracy,
      grid.smartFolderRefresh,
      inspector.inspectorResizeDragging,
      windowHResizing,
      handleGridViewModeChange,
      handleGridSortFieldChange,
      handleGridSortOrderChange,
      handleGridScopeTransitionMidpoint,
      inspector.handleSelectedImagesChange,
      inspector.setSelectionSummarySpec,
      inspector.handleMediaViewStateChange,
      subscriptionRefreshToken,
      grid.setCreateSubscriptionGroupModalOpen,
      viewer,
    ],
  );

  return (
    <div
      className={`${styles.root} ${shellVisible ? styles.shellVisible : styles.shellHidden}`}
    >
      <UpdateBanner />
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
            <KbdTooltip label="Settings" shortcut="Mod+,">
              <button className={`${styles.panelToggleBtn} no-drag-region`} onClick={handleOpenSettings}>
                <IconSettings size={16} />
              </button>
            </KbdTooltip>
            <KbdTooltip label="Subscriptions" shortcut="Mod+Shift+S">
              <button className={`${styles.panelToggleBtn} no-drag-region`} onClick={handleOpenSubscriptions}>
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
              detailViewState={inspector.mediaViewState}
              detailViewControls={inspector.mediaViewControls}
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
              <ViewerHost viewer={viewer} />
            </MainViewModelProvider>
          </ScopedDisplayProvider>
        </div>

        {inspector.showInspector && (
          <InspectorPanel
            selectedImages={inspector.selectedImages}
            selectionSummarySpec={inspector.selectionSummarySpec}
            imageName={inspector.imageName}
            onImageNameChange={inspector.handleNameChange}
            width={settings.inspectorWidth}
            onWidthChange={(w) => updateSetting('inspectorWidth', w)}
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
            onExport={() => useExportActionStore.getState().requestAdvancedExport()}
            refreshMetadata={inspector.refreshMetadata}
          />
        )}
      </div>

      <TagSelectPortal />
      <FolderPickerPortal />
      <AiTaggerPortal />
      <FolderWatchDialog />
      <DragGhost />
      <CreateSubscriptionGroupModal
        opened={grid.createSubscriptionGroupModalOpen}
        onClose={() => grid.setCreateSubscriptionGroupModalOpen(false)}
        onCreated={() => setSubscriptionRefreshToken((v) => v + 1)}
      />
      <CommandPalette open={paletteOpen} onClose={closePalette} mode={paletteMode} actions={paletteActions} />
      {logPanelOpen && <LogPanel onClose={toggleLogPanel} />}
    </div>
  );
}

export default App;
