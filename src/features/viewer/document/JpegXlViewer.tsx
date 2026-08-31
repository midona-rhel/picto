import decode, { init as initJpegXlDecoder } from '@jsquash/jxl/decode.js';
import jpegXlDecoderWasmUrl from '@jsquash/jxl/codec/dec/jxl_dec.wasm?url';
import { useEffect, useMemo, useRef, useState } from 'react';
import type { ViewerZoomControls } from '../../../state/viewer';
import { useImageZoom, type ImageSize } from '../hooks/useImageZoom';
import { useNavigatorDrag } from '../hooks/useNavigatorDrag';
import { useNavigatorRenderer } from '../hooks/useNavigatorRenderer';
import { usePreviewPreferences } from '../usePreviewPreferences';
import styles from './JpegXlViewer.module.css';
import { t } from '../../../i18n';

interface Props {
  src: string;
  thumbnailSrc: string;
  onReady?: () => void;
  onZoomControlsChange?: (controls: ViewerZoomControls | null) => void;
  onZoomPercentChange?: (percent: number) => void;
}

const NAVIGATOR_SIZE = 120;

let decoderReady: Promise<void> | null = null;

function decodeJpegXl(buffer: ArrayBuffer): Promise<ImageData> {
  if (!decoderReady) {
    decoderReady = initJpegXlDecoder({
      locateFile: (path: string) => path.endsWith('.wasm') ? jpegXlDecoderWasmUrl : path,
    }).then(() => undefined).catch((reason) => {
      decoderReady = null;
      throw reason;
    });
  }
  return decoderReady.then(() => decode(buffer));
}

export function JpegXlViewer({ src, thumbnailSrc, onReady, onZoomControlsChange, onZoomPercentChange }: Props) {
  const previewPreferences = usePreviewPreferences();
  const viewportRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const navigatorRef = useRef<HTMLDivElement>(null);
  const navigatorViewportRef = useRef<HTMLDivElement>(null);
  const [image, setImage] = useState<ImageData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const imageSize = useMemo<ImageSize | null>(
    () => image ? { width: image.width, height: image.height } : null,
    [image],
  );
  const imageSizeRef = useRef(imageSize);
  imageSizeRef.current = imageSize;
  const zoom = useImageZoom(viewportRef, imageSize, [frameRef], {
    macTrackpadGestures: previewPreferences.viewerTrackpadGestures,
  });

  useEffect(() => {
    const abort = new AbortController();
    setImage(null);
    setError(null);
    void fetch(src, { signal: abort.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`JPEG XL request failed (${response.status})`);
        return response.arrayBuffer();
      })
      .then(decodeJpegXl)
      .then((decoded) => { if (!abort.signal.aborted) setImage(decoded); })
      .catch((reason: unknown) => {
        if (!abort.signal.aborted) {
          setError(reason instanceof Error ? reason.message : t('Could not open this JPEG XL image.'));
          onReady?.();
        }
      });
    return () => abort.abort();
  }, [onReady, src]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !image) return;
    canvas.width = image.width;
    canvas.height = image.height;
    canvas.getContext('2d')?.putImageData(image, 0, 0);
    onReady?.();
  }, [image, onReady]);

  useEffect(() => {
    if (!imageSize || zoom.containerSize.w === 0 || zoom.containerSize.h === 0) return;
    zoom.fitToWindow();
  }, [imageSize, zoom.containerSize.w, zoom.containerSize.h]); // eslint-disable-line react-hooks/exhaustive-deps

  const controls = useMemo<ViewerZoomControls>(() => ({
    fitToWindow: zoom.fitToWindow,
    fitActual: zoom.fitActual,
    zoomIn: () => zoom.animateZoomTo(zoom.state.scale * 1.25),
    zoomOut: () => zoom.animateZoomTo(zoom.state.scale / 1.25),
    setZoomScale: zoom.zoomTo,
    subscribeZoomScale: zoom.subscribeLiveScale,
  }), [zoom.animateZoomTo, zoom.fitActual, zoom.fitToWindow, zoom.state.scale, zoom.subscribeLiveScale, zoom.zoomTo]);

  useEffect(() => {
    onZoomControlsChange?.(controls);
    return () => onZoomControlsChange?.(null);
  }, [controls, onZoomControlsChange]);

  useEffect(() => onZoomPercentChange?.(Math.round(zoom.state.scale * 100)), [onZoomPercentChange, zoom.state.scale]);

  useNavigatorRenderer(
    navigatorRef,
    navigatorViewportRef,
    imageSizeRef,
    zoom.navigatorRect,
    NAVIGATOR_SIZE,
    zoom.onLiveFrameRef,
    zoom.containerSize,
  );
  const handleNavigatorMouseDown = useNavigatorDrag(navigatorRef, imageSizeRef, zoom.panToNormalized);

  return (
    <div
      ref={viewportRef}
      className={`${styles.viewport} ${zoom.isDragging ? styles.dragging : ''}`}
      onMouseDown={zoom.handlers.onMouseDown}
      data-jpeg-xl-viewer
    >
      {error ? <div className={styles.message} role="alert">{error}</div> : null}
      <div ref={frameRef} className={styles.frame}>
        <canvas ref={canvasRef} className={styles.canvas} />
      </div>
      <div ref={navigatorRef} className={styles.navigator} onMouseDown={handleNavigatorMouseDown} style={{ display: 'none' }}>
        <img src={thumbnailSrc} alt="" draggable={false} className={styles.navigatorImage} />
        <div ref={navigatorViewportRef} className={styles.navigatorViewport} />
      </div>
    </div>
  );
}
