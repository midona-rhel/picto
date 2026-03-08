import { useCallback, useMemo, useRef, useState } from 'react';
import type { DetailViewControls, DetailViewState } from '../../grid/DetailView';
import type { MasonryImageItem } from '../../grid/shared';
import {
  clampSession,
  createSession,
  navigateSession,
  rebaseSession,
  type ViewerSession,
} from '../../grid/runtime/gridViewerSession';

export type { DetailViewControls, DetailViewState } from '../../grid/DetailView';

export type ViewerOverlayMode = 'detail' | 'quick_look' | 'slideshow' | null;

export interface ViewerSource {
  images: MasonryImageItem[];
  totalCount: number | null;
  hasMore: boolean;
  loadMore?: () => void;
  inboxMode?: boolean;
  onInboxAction?: (hash: string, status: 'active' | 'trash') => void;
  onDetailStateChange?: (state: DetailViewState | null, controls: DetailViewControls | null) => void;
  onDetailImageChange?: (hash: string) => void;
  onQuickLookOpen?: (hash: string) => void;
  onQuickLookImageChange?: (hash: string) => void;
  onCloseDetail?: (exitHash: string) => void;
  onCloseQuickLook?: (exitHash: string) => void;
}

export interface ViewerHostController {
  mode: ViewerOverlayMode;
  session: ViewerSession | null;
  source: ViewerSource | null;
  isOpen: boolean;
  isDetailOpen: boolean;
  isQuickLookOpen: boolean;
  registerSource: (source: ViewerSource) => void;
  openDetail: (hash: string) => void;
  toggleQuickLook: (hash: string) => void;
  openSlideshow: (hash?: string | null) => void;
  close: (exitHash?: string) => void;
  navigate: (delta: number) => void;
}

function rebaseOpenSession(session: ViewerSession | null, images: MasonryImageItem[]): ViewerSession | null {
  if (!session) return null;
  const rebased = rebaseSession(session, images);
  if (rebased) return rebased;
  if (images.length === 0) return null;
  return clampSession(session, images);
}

export function useViewerHost(): ViewerHostController {
  const [mode, setMode] = useState<ViewerOverlayMode>(null);
  const [session, setSession] = useState<ViewerSession | null>(null);
  const [sourceVersion, setSourceVersion] = useState(0);
  const sourceRef = useRef<ViewerSource | null>(null);

  const close = useCallback((exitHash = '') => {
    const source = sourceRef.current;
    const currentMode = mode;
    setMode(null);
    setSession(null);
    if (currentMode === 'detail') {
      source?.onDetailStateChange?.(null, null);
      source?.onCloseDetail?.(exitHash);
    } else if (currentMode === 'quick_look') {
      source?.onCloseQuickLook?.(exitHash);
    }
  }, [mode]);

  const registerSource = useCallback((source: ViewerSource) => {
    sourceRef.current = source;
    if (!mode) return;
    setSession((prev) => {
      const next = rebaseOpenSession(prev, source.images);
      if (!next) {
        queueMicrotask(() => close(''));
      }
      return next;
    });
    setSourceVersion((v) => v + 1);
  }, [close, mode]);

  const openDetail = useCallback((hash: string) => {
    const source = sourceRef.current;
    if (!source || source.images.length === 0) return;
    setSession(createSession(source.images, hash));
    setMode('detail');
  }, []);

  const toggleQuickLook = useCallback((hash: string) => {
    const source = sourceRef.current;
    if (!source || source.images.length === 0) return;
    if (mode === 'quick_look') {
      close(hash);
      return;
    }
    setSession(createSession(source.images, hash));
    setMode('quick_look');
    source.onQuickLookOpen?.(hash);
  }, [close, mode]);

  const openSlideshow = useCallback((hash?: string | null) => {
    const source = sourceRef.current;
    if (!source || source.images.length === 0) return;
    const startHash = hash ?? session?.currentHash ?? source.images[0]?.hash ?? null;
    if (!startHash) return;
    setSession(createSession(source.images, startHash));
    setMode('slideshow');
  }, [session?.currentHash]);

  const navigate = useCallback((delta: number) => {
    const source = sourceRef.current;
    if (!source || !session) return;
    const next = navigateSession(session, source.images, delta);
    if (next.currentHash === session.currentHash && next.currentIndex === session.currentIndex) return;
    setSession(next);
    if (mode === 'quick_look') {
      source.onQuickLookImageChange?.(next.currentHash);
      return;
    }
    if (mode === 'detail') {
      source.onDetailImageChange?.(next.currentHash);
    }
  }, [mode, session]);

  return useMemo(() => ({
    mode,
    session,
    source: sourceRef.current,
    isOpen: mode !== null,
    isDetailOpen: mode === 'detail',
    isQuickLookOpen: mode === 'quick_look',
    registerSource,
    openDetail,
    toggleQuickLook,
    openSlideshow,
    close,
    navigate,
  }), [
    close,
    mode,
    navigate,
    openDetail,
    openSlideshow,
    registerSource,
    session,
    sourceVersion,
    toggleQuickLook,
  ]);
}
