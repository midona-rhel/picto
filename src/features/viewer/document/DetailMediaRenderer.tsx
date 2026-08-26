import { lazy, Suspense, useCallback, useState, type MouseEvent, type ReactNode } from 'react';
import { mediaFileUrl, mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import { ThumbnailImage } from '../../../shared/ui/ThumbnailImage/ThumbnailImage';
import { VideoPlayer } from '../video/VideoPlayer';
import { detailRendererKind } from './detailRendererKind';
import type { FlashPlaybackController } from './FlashPlayer';
import type { CurrentFrameCapture } from '../currentFrameCapture';
import type { ViewerZoomControls } from '../../../state/viewer';
import { UnsupportedDocumentViewer } from './UnsupportedDocumentViewer';
import { ProgressiveMediaFrame } from '../ProgressiveMediaFrame';
import styles from './DetailMediaRenderer.module.css';

const FlashPlayer = lazy(() => import('./FlashPlayer').then((module) => ({ default: module.FlashPlayer })));
const PdfViewer = lazy(() => import('./PdfViewer').then((module) => ({ default: module.PdfViewer })));
const FontViewer = lazy(() => import('./FontViewer').then((module) => ({ default: module.FontViewer })));
const JpegXlViewer = lazy(() => import('./JpegXlViewer').then((module) => ({ default: module.JpegXlViewer })));
const TextDocumentViewer = lazy(() => import('./TextDocumentViewer').then((module) => ({ default: module.TextDocumentViewer })));
const DocxViewer = lazy(() => import('./DocxViewer').then((module) => ({ default: module.DocxViewer })));
const PptxViewer = lazy(() => import('./PptxViewer').then((module) => ({ default: module.PptxViewer })));
const ArchiveDocumentViewer = lazy(() => import('./ArchiveDocumentViewer').then((module) => ({ default: module.ArchiveDocumentViewer })));
const DjvuViewer = lazy(() => import('./DjvuViewer').then((module) => ({ default: module.DjvuViewer })));

interface Props {
  hash: string;
  mimeType: string;
  displayName?: string | null;
  onFlashPlaybackChange?: (controller: FlashPlaybackController | null) => void;
  onFlashContextMenu?: (event: MouseEvent) => void;
  onFrameCaptureChange?: (capture: CurrentFrameCapture | null) => void;
  onPdfZoomControlsChange?: (controller: ViewerZoomControls | null) => void;
  onPdfZoomPercentChange?: (percent: number) => void;
  mediaKeyboardShortcutsEnabled?: boolean;
  mediaAutoPlay?: boolean;
  mediaLoop?: boolean;
  mediaMuted?: boolean;
  onReady?: () => void;
}

export function DetailMediaRenderer({ hash, mimeType, displayName, onFlashPlaybackChange, onFlashContextMenu, onFrameCaptureChange, onPdfZoomControlsChange, onPdfZoomPercentChange, mediaKeyboardShortcutsEnabled = true, mediaAutoPlay, mediaLoop, mediaMuted, onReady }: Props) {
  const kind = detailRendererKind(mimeType);
  const src = mediaFileUrl(hash, mimeType);
  const identity = `${hash}:${mimeType}`;
  const [readyIdentity, setReadyIdentity] = useState<string | null>(null);
  const ready = readyIdentity === identity;
  const markReady = useCallback(() => {
    setReadyIdentity(identity);
    onReady?.();
  }, [identity, onReady]);
  const usesRendererSnapshot = kind === 'text-document'
    || kind === 'docx'
    || kind === 'pptx'
    || kind === 'epub'
    || kind === 'cbz'
    || kind === 'djvu';
  let renderer: ReactNode;
  if (kind === 'audio') {
    renderer = <VideoPlayer key={hash} kind="audio" src={src} waveformSrc={mediaThumbnailUrl(hash)} autoPlay={mediaAutoPlay} loop={mediaLoop} muted={mediaMuted ?? false} keyboardShortcutsEnabled={mediaKeyboardShortcutsEnabled} onReady={markReady} />;
  } else if (kind === 'video') {
    renderer = <VideoPlayer key={hash} src={src} autoPlay={mediaAutoPlay} loop={mediaLoop} muted={mediaMuted} onFrameCaptureChange={onFrameCaptureChange} keyboardShortcutsEnabled={mediaKeyboardShortcutsEnabled} onReady={markReady} />;
  } else if (kind === 'jpeg-xl') {
    renderer = <Suspense fallback={null}><JpegXlViewer key={hash} src={src} thumbnailSrc={mediaThumbnailUrl(hash)} onZoomControlsChange={onPdfZoomControlsChange} onZoomPercentChange={onPdfZoomPercentChange} onReady={markReady} /></Suspense>;
  } else if (kind === 'pdf') {
    renderer = <Suspense fallback={null}><PdfViewer key={hash} src={src} onZoomControlsChange={onPdfZoomControlsChange} onZoomPercentChange={onPdfZoomPercentChange} onReady={markReady} /></Suspense>;
  } else if (kind === 'text-document') {
    renderer = <Suspense fallback={null}><TextDocumentViewer key={hash} src={src} mimeType={mimeType} onReady={markReady} /></Suspense>;
  } else if (kind === 'docx') {
    renderer = <Suspense fallback={null}><DocxViewer key={hash} src={src} onReady={markReady} /></Suspense>;
  } else if (kind === 'pptx') {
    renderer = <Suspense fallback={null}><PptxViewer key={hash} src={src} onReady={markReady} /></Suspense>;
  } else if (kind === 'epub' || kind === 'cbz') {
    renderer = <Suspense fallback={null}><ArchiveDocumentViewer key={hash} src={src} kind={kind} onReady={markReady} /></Suspense>;
  } else if (kind === 'djvu') {
    renderer = <Suspense fallback={null}><DjvuViewer key={hash} src={src} onReady={markReady} /></Suspense>;
  } else if (kind === 'unsupported') {
    return <UnsupportedDocumentViewer mimeType={mimeType} />;
  } else if (kind === 'font') {
    renderer = (
      <Suspense fallback={null}>
        <FontViewer key={hash} src={src} displayName={displayName ?? 'Font preview'} mimeType={mimeType} onReady={markReady} />
      </Suspense>
    );
  } else if (kind === 'flash') {
    renderer = (
      <Suspense fallback={null}>
        <FlashPlayer
          key={hash}
          src={src}
          onPlaybackChange={onFlashPlaybackChange}
          onContextMenu={onFlashContextMenu}
          onFrameCaptureChange={onFrameCaptureChange}
          onReady={markReady}
        />
      </Suspense>
    );
  } else {
    return null;
  }

  const preview = kind === 'video' ? (
    <div className={styles.videoPreview} data-video-frame-preview>
      <ThumbnailImage
        className={styles.videoPreviewImage}
        src={mediaThumbnailUrl(hash)}
        fallback="broken"
        alt=""
        draggable={false}
      />
    </div>
  ) : kind === 'pdf' ? (
    <div className={styles.documentPreview} data-document-page-preview>
      <div className={styles.documentPreviewViewport}>
        <ThumbnailImage
          className={styles.documentPreviewImage}
          src={mediaThumbnailUrl(hash)}
          fallback="broken"
          alt=""
          draggable={false}
        />
      </div>
      <div className={styles.documentPreviewFooter} />
    </div>
  ) : usesRendererSnapshot ? (
    <div className={styles.rendererSnapshotPreview} data-document-renderer-snapshot>
      <ThumbnailImage
        className={styles.rendererSnapshotImage}
        src={mediaThumbnailUrl(hash)}
        fallback="broken"
        alt=""
        draggable={false}
      />
    </div>
  ) : kind === 'flash' ? (
    <div className={styles.flashPreview} data-flash-stage-preview>
      <ThumbnailImage
        className={styles.flashPreviewImage}
        src={mediaThumbnailUrl(hash)}
        fallback="broken"
        alt=""
        draggable={false}
      />
    </div>
  ) : (
    <div className={styles.preview}>
      <ThumbnailImage
        className={styles.previewImage}
        src={mediaThumbnailUrl(hash)}
        fallback={kind === 'font' ? 'font' : 'broken'}
        alt=""
        draggable={false}
      />
    </div>
  );

  return (
    <div className={styles.frame} data-progressive-media-renderer data-ready={ready ? 'true' : 'false'}>
      <ProgressiveMediaFrame
        className={styles.progressiveFrame}
        preview={preview}
        previewVisible={!ready}
        contentReady={ready}
      >
        {renderer}
      </ProgressiveMediaFrame>
    </div>
  );
}
