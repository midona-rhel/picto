import { useState, useRef, useCallback, useEffect } from 'react';
import { ActionIcon, Loader } from '@mantine/core';
import { KbdTooltip } from './ui/KbdTooltip';
import { IconPlus, IconMinus, IconArrowsMinimize, IconArrowsMaximize } from '@tabler/icons-react';
import { useSettingsStore } from '../stores/settingsStore';
import { mediaThumbnailUrl } from '../lib/mediaUrl';

interface ZoomableImageProps {
  src: string;
  alt?: string;
  hash?: string; // For minimap thumbnail
}

export function ZoomableImage({ src, alt, hash }: ZoomableImageProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  const [naturalW, setNaturalW] = useState(0);
  const [naturalH, setNaturalH] = useState(0);
  const [containerW, setContainerW] = useState(0);
  const [containerH, setContainerH] = useState(0);
  const [ready, setReady] = useState(false);

  const [scale, setScale] = useState(1);
  const [tx, setTx] = useState(0);
  const [ty, setTy] = useState(0);

  const dragRef = useRef<{ sx: number; sy: number; stx: number; sty: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [animate, setAnimate] = useState(false);

  const measureContainer = useCallback(() => {
    const c = containerRef.current;
    if (!c) return;
    setContainerW(c.clientWidth);
    setContainerH(c.clientHeight);
  }, []);

  useEffect(() => {
    measureContainer();
  }, [measureContainer]);

  useEffect(() => {
    const onResize = () => measureContainer();
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, [measureContainer]);

  useEffect(() => {
    let cancelled = false;
    setReady(false);
    setNaturalW(0);
    setNaturalH(0);

    const img = new Image();
    img.onload = () => {
      if (cancelled) return;
      setNaturalW(img.naturalWidth);
      setNaturalH(img.naturalHeight);
      measureContainer();
      setReady(true);
    };
    img.onerror = () => {
      if (cancelled) return;
      setReady(false);
    };
    img.src = src;

    return () => {
      cancelled = true;
    };
  }, [src, measureContainer]);

  const containScale = containerW > 0 && naturalW > 0
    ? Math.min(containerW / naturalW, containerH / naturalH)
    : 1;
  const coverScale = containerW > 0 && naturalW > 0
    ? Math.max(containerW / naturalW, containerH / naturalH)
    : 1;

  useEffect(() => {
    if (!ready) return;
    setScale(containScale);
    setTx(0);
    setTy(0);
  }, [ready, containScale]);

  const clamp = useCallback((x: number, y: number, s: number): [number, number] => {
    const iw = naturalW * s;
    const ih = naturalH * s;
    let cx = x;
    let cy = y;
    if (iw <= containerW) {
      cx = 0;
    } else {
      const m = (iw - containerW) / 2;
      cx = Math.max(-m, Math.min(m, cx));
    }
    if (ih <= containerH) {
      cy = 0;
    } else {
      const m = (ih - containerH) / 2;
      cy = Math.max(-m, Math.min(m, cy));
    }
    return [cx, cy];
  }, [naturalW, naturalH, containerW, containerH]);

  const handleWheel = useCallback((e: React.WheelEvent) => {
    if (!ready) return;
    e.stopPropagation();
    e.preventDefault();

    const clampedDelta = Math.max(-120, Math.min(120, e.deltaY));
    const factor = Math.exp(-clampedDelta * 0.0025);
    const minS = containScale;
    const maxS = Math.max(coverScale * 6, 6);

    setAnimate(false);
    setScale(prev => {
      const next = Math.max(minS, Math.min(maxS, prev * factor));
      const c = containerRef.current;
      if (!c || prev <= 0 || next <= 0) {
        const [cx, cy] = clamp(tx, ty, next);
        setTx(cx);
        setTy(cy);
        return next;
      }

      const rect = c.getBoundingClientRect();
      const mx = e.clientX - rect.left - rect.width / 2;
      const my = e.clientY - rect.top - rect.height / 2;
      const worldX = (mx - tx) / prev;
      const worldY = (my - ty) / prev;
      const nextTx = mx - worldX * next;
      const nextTy = my - worldY * next;
      const [cx, cy] = clamp(nextTx, nextTy, next);
      setTx(cx);
      setTy(cy);
      return next;
    });
  }, [ready, containScale, coverScale, clamp, tx, ty]);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    if (!ready) return;
    e.stopPropagation();
    e.preventDefault();
    setAnimate(false);
    setDragging(true);
    dragRef.current = { sx: e.clientX, sy: e.clientY, stx: tx, sty: ty };
  }, [ready, tx, ty]);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (e: MouseEvent) => {
      const d = dragRef.current;
      if (!d) return;
      const [cx, cy] = clamp(d.stx + e.clientX - d.sx, d.sty + e.clientY - d.sy, scale);
      setTx(cx);
      setTy(cy);
    };
    const onUp = () => {
      setDragging(false);
      dragRef.current = null;
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [dragging, scale, clamp]);

  const handleDblClick = useCallback((e: React.MouseEvent) => {
    if (!ready) return;
    e.stopPropagation();
    e.preventDefault();
    setAnimate(true);
    const nearCover = Math.abs(scale - coverScale) < Math.abs(scale - containScale);
    setScale(nearCover ? containScale : coverScale);
    setTx(0);
    setTy(0);
  }, [ready, scale, coverScale, containScale]);

  const onZoomIn = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setAnimate(true);
    const maxS = Math.max(coverScale * 6, 6);
    setScale(prev => {
      const next = Math.min(maxS, prev * 1.25);
      const [cx, cy] = clamp(tx, ty, next);
      setTx(cx);
      setTy(cy);
      return next;
    });
  }, [coverScale, clamp, tx, ty]);

  const onZoomOut = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setAnimate(true);
    const minS = containScale;
    setScale(prev => {
      const next = Math.max(minS, prev / 1.25);
      const [cx, cy] = clamp(tx, ty, next);
      setTx(cx);
      setTy(cy);
      return next;
    });
  }, [containScale, clamp, tx, ty]);

  const onToggleFit = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    setAnimate(true);
    const nearCover = Math.abs(scale - coverScale) < Math.abs(scale - containScale);
    setScale(nearCover ? containScale : coverScale);
    setTx(0);
    setTy(0);
  }, [scale, coverScale, containScale]);

  const nearCover = Math.abs(scale - coverScale) < Math.abs(scale - containScale);

  if (!ready) {
    return (
      <div
        ref={containerRef}
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          background: 'var(--color-black-99)',
        }}
      >
        <Loader size="md" />
      </div>
    );
  }

  const showMinimap = useSettingsStore(s => s.settings.showMinimap);

  const dw = naturalW * scale;
  const dh = naturalH * scale;
  const imgLeft = (containerW - dw) / 2 + tx;
  const imgTop = (containerH - dh) / 2 + ty;

  const isZoomedIn = scale > containScale * 1.01;
  const MINIMAP_MAX = 120;
  const minimapScale = naturalW > 0 ? Math.min(MINIMAP_MAX / naturalW, MINIMAP_MAX / naturalH) : 0;
  const mmW = naturalW * minimapScale;
  const mmH = naturalH * minimapScale;

  const visX = Math.max(0, -imgLeft / scale);
  const visY = Math.max(0, -imgTop / scale);
  const visW = Math.min(naturalW, containerW / scale);
  const visH = Math.min(naturalH, containerH / scale);

  const rectX = visX * minimapScale;
  const rectY = visY * minimapScale;
  const rectW = Math.min(mmW, visW * minimapScale);
  const rectH = Math.min(mmH, visH * minimapScale);

  const handleMinimapClick = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    e.stopPropagation();
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    // Convert to image coords and center viewport there
    const imageX = mx / minimapScale;
    const imageY = my / minimapScale;
    const newTx = scale * (naturalW / 2 - imageX);
    const newTy = scale * (naturalH / 2 - imageY);
    const [cx, cy] = clamp(newTx, newTy, scale);
    setAnimate(false);
    setTx(cx);
    setTy(cy);
  }, [minimapScale, scale, naturalW, naturalH, clamp]);

  return (
    <div
      ref={containerRef}
      onClick={(e) => e.stopPropagation()}
      onWheel={handleWheel}
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDblClick}
      style={{
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        position: 'relative',
        cursor: dragging ? 'grabbing' : 'grab',
        userSelect: 'none',
        background: 'var(--color-black-99)',
      }}
    >
      <img
        src={src}
        alt={alt}
        draggable={false}
        style={{
          position: 'absolute',
          left: imgLeft,
          top: imgTop,
          width: dw,
          height: dh,
          pointerEvents: 'none',
          transition: animate
            ? 'left 0.2s ease-out, top 0.2s ease-out, width 0.2s ease-out, height 0.2s ease-out'
            : 'none',
        }}
      />

      {/* Minimap overlay */}
      {showMinimap && isZoomedIn && mmW > 0 && (
        <div
          onClick={handleMinimapClick}
          onMouseDown={(e) => e.stopPropagation()}
          style={{
            position: 'absolute',
            bottom: 16,
            left: 16,
            width: mmW,
            height: mmH,
            borderRadius: 4,
            overflow: 'hidden',
            background: 'rgba(0,0,0,0.6)',
            border: '1px solid rgba(255,255,255,0.2)',
            cursor: 'crosshair',
            zIndex: 10,
          }}
        >
          <img
            src={hash ? mediaThumbnailUrl(hash) : src}
            alt=""
            draggable={false}
            style={{
              width: mmW,
              height: mmH,
              objectFit: 'contain',
              pointerEvents: 'none',
              opacity: 0.8,
            }}
          />
          <div
            style={{
              position: 'absolute',
              left: rectX,
              top: rectY,
              width: rectW,
              height: rectH,
              border: '1.5px solid var(--color-primary, #3b82f6)',
              background: 'rgba(59, 130, 246, 0.15)',
              pointerEvents: 'none',
            }}
          />
        </div>
      )}

      <div
        style={{
          position: 'absolute',
          bottom: 16,
          right: 16,
          display: 'flex',
          gap: 4,
          background: 'var(--color-black-50)',
          borderRadius: 8,
          padding: 4,
          zIndex: 10,
        }}
      >
        <KbdTooltip label="Zoom in" position="top">
          <ActionIcon variant="subtle" color="gray" size="md" onClick={onZoomIn}>
            <IconPlus size={16} color="var(--color-text-primary)" />
          </ActionIcon>
        </KbdTooltip>
        <KbdTooltip label="Zoom out" position="top">
          <ActionIcon variant="subtle" color="gray" size="md" onClick={onZoomOut}>
            <IconMinus size={16} color="var(--color-text-primary)" />
          </ActionIcon>
        </KbdTooltip>
        <KbdTooltip label={nearCover ? 'Fit to screen' : 'Fill screen'} position="top">
          <ActionIcon variant="subtle" color="gray" size="md" onClick={onToggleFit}>
            {nearCover ? <IconArrowsMinimize size={16} color="var(--color-text-primary)" /> : <IconArrowsMaximize size={16} color="var(--color-text-primary)" />}
          </ActionIcon>
        </KbdTooltip>
      </div>
    </div>
  );
}
