/**
 * QuickLook — peek overlay (Space key).
 * Thin wrapper around DetailView: provides the overlay chrome (backdrop, exit button,
 * nav toolbar) while DetailView handles all image rendering, zoom, transitions,
 * collections, and keyboard shortcuts.
 */
import { useState, useEffect, useRef, useCallback } from 'react';
import { IconChevronLeft, IconChevronRight, IconX } from '@tabler/icons-react';
import type { MasonryImageItem } from './shared';
import { DetailView, type DetailViewState, type DetailViewControls } from './DetailView';
import { KbdTooltip } from '../ui/KbdTooltip';
import { useGlobalKeydown } from '../../hooks/useGlobalKeydown';
import styles from './QuickLook.module.css';

interface QuickLookProps {
  images: MasonryImageItem[];
  currentIndex: number;
  onNavigate: (delta: number) => void;
  onClose: (exitHash: string) => void;
  onImageChange?: (hash: string) => void;
  onLoadMore?: () => void;
  /** Actual total count of images in the scope (may exceed images.length if not all pages are loaded) */
  totalCount?: number | null;
}

export function QuickLook({ images, currentIndex, onNavigate, onClose, onImageChange, onLoadMore, totalCount }: QuickLookProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [viewState, setViewState] = useState<DetailViewState | null>(null);
  const controlsRef = useRef<DetailViewControls | null>(null);

  useEffect(() => {
    const raf = requestAnimationFrame(() => setIsOpen(true));
    return () => cancelAnimationFrame(raf);
  }, []);

  const handleStateChange = useCallback((state: DetailViewState, controls: DetailViewControls) => {
    setViewState(state);
    controlsRef.current = controls;
  }, []);

  const handleQuickLookHotkeys = useCallback((e: KeyboardEvent) => {
    if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
    const ctrl = controlsRef.current;
    if (!ctrl) return;
    switch (e.key) {
      case '=':
      case '+':
        e.preventDefault();
        ctrl.setZoomScale((viewState?.zoomScale ?? 1) * 1.25);
        break;
      case '-':
        if (!e.metaKey && !e.ctrlKey) {
          e.preventDefault();
          ctrl.setZoomScale((viewState?.zoomScale ?? 1) / 1.25);
        }
        break;
      case '`':
        e.preventDefault();
        ctrl.fitToWindow();
        break;
      case '0':
        if (e.metaKey || e.ctrlKey) {
          e.preventDefault();
          ctrl.fitActual();
        }
        break;
    }
  }, [viewState?.zoomScale]);
  useGlobalKeydown(handleQuickLookHotkeys);

  return (
    <div className={styles.overlay}>
      <KbdTooltip label="Close" shortcut="Space">
        <button
          className={styles.exitBtn}
          onClick={() => controlsRef.current?.close()}
        >
          <IconX size={16} />
        </button>
      </KbdTooltip>

      <div className={`${styles.imageArea} ${isOpen ? styles.open : ''}`}>
        <DetailView
          images={images}
          currentIndex={currentIndex}
          onNavigate={onNavigate}
          onClose={onClose}
          onStateChange={handleStateChange}
          onImageChange={onImageChange}
          onLoadMore={onLoadMore}
          totalCount={totalCount}
        />
      </div>

      <div className={styles.inlineToolbar}>
        <KbdTooltip label="Previous" shortcut="ArrowLeft">
          <button
            className={styles.navBtn}
            onClick={() => controlsRef.current?.navigate(-1)}
            disabled={currentIndex === 0}
          >
            <IconChevronLeft size={18} />
          </button>
        </KbdTooltip>

        <span className={styles.pageCounter}>
          {viewState
            ? `${viewState.currentIndex + 1} / ${viewState.total}`
            : `${currentIndex + 1} / ${totalCount ?? images.length}`}
        </span>

        <KbdTooltip label="Next" shortcut="ArrowRight">
          <button
            className={styles.navBtn}
            onClick={() => controlsRef.current?.navigate(1)}
            disabled={currentIndex === images.length - 1}
          >
            <IconChevronRight size={18} />
          </button>
        </KbdTooltip>
      </div>
    </div>
  );
}
