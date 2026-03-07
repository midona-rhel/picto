import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '#desktop/api';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';

import { useInspectorData, type InspectorData } from '../../../hooks/useInspectorData';
import type { MasonryImageItem } from '../../../components/image-grid/shared';
import type { DetailViewState, DetailViewControls } from '../../../components/image-grid/DetailView';
import type { SelectionQuerySpec } from '../../../components/image-grid/metadataPrefetch';

export interface InspectorStateParams {
  showInspectorSetting: boolean;
  currentView: string;
  propertiesPanelWidth: number;
}

export interface InspectorState extends InspectorData {
  selectedImages: MasonryImageItem[];
  handleSelectedImagesChange: (images: MasonryImageItem[]) => void;
  selectionSummarySpec: SelectionQuerySpec | null;
  setSelectionSummarySpec: (spec: SelectionQuerySpec | null) => void;
  imageName: string;
  handleNameChange: (name: string) => void;
  detailViewState: DetailViewState | null;
  detailViewControls: DetailViewControls | null;
  handleDetailViewStateChange: (state: DetailViewState | null, controls: DetailViewControls | null) => void;
  inspectorResizeDragging: boolean;
  setInspectorResizeDragging: (v: boolean) => void;
  showInspector: boolean;
  inspectorWidth: number;
  isDetailMode: boolean;
  isPinned: boolean;
  togglePin: () => void;
}

function isSameDetailViewState(a: DetailViewState | null, b: DetailViewState | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  const EPSILON = 0.0001;
  return (
    a.currentIndex === b.currentIndex &&
    a.total === b.total &&
    a.zoomPercent === b.zoomPercent &&
    Math.abs(a.zoomScale - b.zoomScale) <= EPSILON &&
    Math.abs(a.fitScale - b.fitScale) <= EPSILON &&
    a.isStripMode === b.isStripMode
  );
}

function isSameDetailViewControls(a: DetailViewControls | null, b: DetailViewControls | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.close === b.close &&
    a.navigate === b.navigate &&
    a.setZoomScale === b.setZoomScale &&
    a.fitToWindow === b.fitToWindow &&
    a.fitActual === b.fitActual
  );
}

export function useInspectorState({
  showInspectorSetting,
  currentView,
  propertiesPanelWidth,
}: InspectorStateParams): InspectorState {
  const [selectedImages, setSelectedImages] = useState<MasonryImageItem[]>([]);
  const [selectionSummarySpec, setSelectionSummarySpec] = useState<SelectionQuerySpec | null>(null);
  const [imageName, setImageName] = useState('');
  const [isPinned, setIsPinned] = useState(false);

  const selectedImageRef = useRef<MasonryImageItem | null>(null);
  selectedImageRef.current = selectedImages.length === 1 ? selectedImages[0] : null;
  const saveNameTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingNameChangeRef = useRef<{ hash: string; before: string | null } | null>(null);

  const togglePin = useCallback(() => setIsPinned((v) => !v), []);

  const handleSelectedImagesChange = useCallback((images: MasonryImageItem[]) => {
    if (isPinned) return;
    if (saveNameTimer.current) {
      clearTimeout(saveNameTimer.current);
      saveNameTimer.current = null;
      pendingNameChangeRef.current = null;
    }
    setSelectedImages(images);
    if (images.length > 0) setSelectionSummarySpec(null);
    if (images.length === 1) {
      const image = images[0];
      setImageName(image.name || image.hash.slice(0, 8));
    } else {
      setImageName('');
    }
  }, [isPinned]);

  const handleNameChange = useCallback((name: string) => {
    setImageName(name);
    if (saveNameTimer.current) clearTimeout(saveNameTimer.current);
    const selected = selectedImageRef.current;
    if (selected?.is_collection && selected.entity_id != null) {
      const collectionId = selected.entity_id;
      const nextName = (name || '').trim() || (selected.name ?? `Collection ${collectionId}`);
      const beforeName = selected.name ?? `Collection ${collectionId}`;
      saveNameTimer.current = setTimeout(() => {
        if (beforeName === nextName) return;
        api.collections.update({ id: collectionId, name: nextName })
          .then(() => {
            registerUndoAction({
              label: 'Rename collection',
              undo: () => api.collections.update({ id: collectionId, name: beforeName }),
              redo: () => api.collections.update({ id: collectionId, name: nextName }),
            });
          })
          .catch((e: unknown) => {
            console.error('Failed to save collection name:', e);
          });
      }, 500);
      return;
    }
    const hash = selected?.hash;
    if (!hash) return;
    const nextName = name || null;
    const pending = pendingNameChangeRef.current;
    if (!pending || pending.hash !== hash) {
      pendingNameChangeRef.current = { hash, before: selected?.name ?? null };
    }
    saveNameTimer.current = setTimeout(() => {
      const before = pendingNameChangeRef.current?.hash === hash
        ? pendingNameChangeRef.current.before
        : (selected?.name ?? null);
      if (before === nextName) {
        pendingNameChangeRef.current = null;
        return;
      }
      api.file.setName(hash, nextName)
        .then(() => {
          registerUndoAction({
            label: 'Rename file',
            undo: () => api.file.setName(hash, before),
            redo: () => api.file.setName(hash, nextName),
          });
        })
        .catch((e: unknown) => {
          console.error('Failed to save name:', e);
        })
        .finally(() => {
          if (pendingNameChangeRef.current?.hash === hash) {
            pendingNameChangeRef.current = null;
          }
        });
    }, 500);
  }, []);

  useEffect(() => () => {
    if (saveNameTimer.current) {
      clearTimeout(saveNameTimer.current);
      saveNameTimer.current = null;
    }
  }, []);

  const [detailViewState, setDetailViewState] = useState<DetailViewState | null>(null);
  const [detailViewControls, setDetailViewControls] = useState<DetailViewControls | null>(null);
  const handleDetailViewStateChange = useCallback((state: DetailViewState | null, controls: DetailViewControls | null) => {
    setDetailViewState((prev) => (isSameDetailViewState(prev, state) ? prev : state));
    setDetailViewControls((prev) => (isSameDetailViewControls(prev, controls) ? prev : controls));
  }, []);

  const [inspectorResizeDragging, setInspectorResizeDragging] = useState(false);
  const showInspector = showInspectorSetting && currentView === 'images';
  const inspectorWidth = showInspector ? propertiesPanelWidth : 0;
  const isDetailMode = !!detailViewState;

  useEffect(() => {
    if (!showInspector && inspectorResizeDragging) {
      setInspectorResizeDragging(false);
    }
  }, [showInspector, inspectorResizeDragging]);

  useEffect(() => {
    document.documentElement.style.setProperty('--inspector-width', inspectorWidth + 'px');
  }, [inspectorWidth]);

  const inspectorData = useInspectorData(selectedImages, selectionSummarySpec);

  return {
    selectedImages,
    handleSelectedImagesChange,
    selectionSummarySpec,
    setSelectionSummarySpec,
    imageName,
    handleNameChange,
    detailViewState,
    detailViewControls,
    handleDetailViewStateChange,
    inspectorResizeDragging,
    setInspectorResizeDragging,
    showInspector,
    inspectorWidth,
    isDetailMode,
    isPinned,
    togglePin,
    ...inspectorData,
  };
}
