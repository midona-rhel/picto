import { useCallback, useMemo, useRef, useState } from 'react';
import type { MediaViewControls, MediaViewState } from '../components/MediaView';
import type { MediaItem } from '../../grid/shared';
import {
  clampSession,
  createSession,
  navigateSession,
  rebaseSession,
  type ViewerSession,
} from '../../grid/runtime/gridViewerSession';

export type { MediaViewControls, MediaViewState } from '../components/MediaView';

export type ViewerOverlayMode = 'detail' | 'quick_look' | 'slideshow' | null;

export interface ViewerSource {
  images: MediaItem[];
  totalCount: number | null;
  inboxMode?: boolean;
  onInboxAction?: (hash: string, status: 'active' | 'trash') => void;
  onDetailStateChange?: (state: MediaViewState | null, controls: MediaViewControls | null) => void;
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

function rebaseOpenSession(session: ViewerSession | null, images: MediaItem[]): ViewerSession | null {
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
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const sessionRef = useRef(session);
  sessionRef.current = session;

  const close = useCallback((exitHash = '') => {
    const source = sourceRef.current;
    const currentMode = modeRef.current;
    setMode(null);
    setSession(null);
    if (currentMode === 'detail') {
      source?.onDetailStateChange?.(null, null);
      source?.onCloseDetail?.(exitHash);
    } else if (currentMode === 'quick_look') {
      source?.onCloseQuickLook?.(exitHash);
    }
  }, []);

  const registerSource = useCallback((source: ViewerSource) => {
    sourceRef.current = source;
    if (!modeRef.current) return;
    setSession((prev) => {
      if (!prev) return prev; // no active session to rebase
      const next = rebaseOpenSession(prev, source.images);
      if (!next) {
        queueMicrotask(() => close(''));
      }
      return next;
    });
    setSourceVersion((v) => v + 1);
  }, [close]);

  const openDetail = useCallback((hash: string) => {
    const source = sourceRef.current;
    if (!source || source.images.length === 0) return;
    setSession(createSession(source.images, hash));
    setMode('detail');
  }, []);

  const toggleQuickLook = useCallback((hash: string) => {
    const source = sourceRef.current;
    if (!source || source.images.length === 0) return;
    if (modeRef.current === 'quick_look') {
      close(hash);
      return;
    }
    setSession(createSession(source.images, hash));
    setMode('quick_look');
    source.onQuickLookOpen?.(hash);
  }, [close]);

  const openSlideshow = useCallback((hash?: string | null) => {
    const source = sourceRef.current;
    if (!source || source.images.length === 0) return;
    const startHash = hash ?? sessionRef.current?.currentHash ?? source.images[0]?.hash ?? null;
    if (!startHash) return;
    setSession(createSession(source.images, startHash));
    setMode('slideshow');
  }, []);

  const navigate = useCallback((delta: number) => {
    const source = sourceRef.current;
    const currentSession = sessionRef.current;
    if (!source || !currentSession) return;
    const next = navigateSession(currentSession, source.images, delta);
    if (next.currentHash === currentSession.currentHash && next.currentIndex === currentSession.currentIndex) return;
    setSession(next);
    const currentMode = modeRef.current;
    if (currentMode === 'quick_look') {
      source.onQuickLookImageChange?.(next.currentHash);
      return;
    }
    if (currentMode === 'detail') {
      source.onDetailImageChange?.(next.currentHash);
    }
  }, []);

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
