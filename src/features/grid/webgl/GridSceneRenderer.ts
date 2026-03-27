import {
  Container,
  Graphics,
  Sprite,
  Text,
  TextStyle,
  Texture,
  type Application,
} from './pixiRuntime';
import type { LayoutItem } from '../layout/types';
import type { CanvasRenderItem } from '../canvas/renderItemAdapter';
import {
  formatDuration,
  getContainRect,
  isHiddenBadgeType,
  mimeToExt,
} from '../canvas/primitives';
import type { ScrollPlatformProfile, ScrollbarVisualState } from './GridScrollController';
import type { TextureEntry, GridTextureStore } from './GridTextureStore';

const PLACEHOLDER_BG_COLOR = 0xffffff;
const PLACEHOLDER_BG_ALPHA = 0.04;
const BORDER_COLOR = 0xffffff;
const RADIUS = 8;
const BADGE_GAP = 4;
const ACTIVATION_DWELL_MS = 30;
const REVEAL_FADE_MS = 250;
const ENABLE_BENCH_LOGS = false;
const ENABLE_ACTIVE_FRAME_LOGS = true;
const ACTIVE_FRAME_LOG_MS = 6;
const badgeTextStyle = new TextStyle({
  fontFamily: '-apple-system, BlinkMacSystemFont, Segoe UI, sans-serif',
  fontSize: 10,
  fontWeight: '600',
  fill: 0xffffff,
});

const nameTextStyle = new TextStyle({
  fontFamily: '-apple-system, BlinkMacSystemFont, Segoe UI, sans-serif',
  fontSize: 13,
  fill: 0xffffff,
});

const ratingTextStyle = new TextStyle({
  fontFamily: 'sans-serif',
  fontSize: 10,
  fill: 0xffd54f,
});

interface BadgeView {
  container: Container;
  background: Graphics;
  text: Text;
}

interface TileView {
  container: Container;
  mediaContainer: Container;
  maskSprite: Sprite;
  frame: Graphics;
  outerPlaceholder: Graphics;
  innerPlaceholder: Graphics;
  hover: Graphics;
  sprite: Sprite;
  durationBadge: BadgeView;
  collectionBadge: BadgeView;
  extensionBadge: BadgeView;
  indexBadge: BadgeView;
  ratingText: Text;
  nameText: Text;
  boundIndex: number | null;
  revealKey: string;
  chromeKey: string;
  spriteKey: string;
  maskKey: string;
  hoverKey: string;
}

type RevealStatus = 'activation_pending' | 'activation_ready' | 'fading' | 'shown';

interface RevealState {
  status: RevealStatus;
  activationSince: number;
  fadeStartedAt: number | null;
}

export interface SceneTileSnapshot {
  index: number;
  itemHash: string;
  thumbnailHash: string;
  renderItem: CanvasRenderItem;
  position: LayoutItem;
  isVisible: boolean;
  isInActivationRange: boolean;
  showName: boolean;
  showExtension: boolean;
  viewMode: 'waterfall' | 'grid' | 'justified';
  suppressTileReveal: boolean;
  textureEntry?: TextureEntry | null;
  hovered: boolean;
  showStressIndex: boolean;
}

export interface GridSceneSnapshot {
  viewportWidth: number;
  viewportHeight: number;
  platform: ScrollPlatformProfile;
  scrollbar: ScrollbarVisualState;
  tiles: SceneTileSnapshot[];
}

export interface GridScenePerfSample {
  tickerFps: number;
  workFps: number;
  activeTileCount: number;
  snapshotTileCount: number;
  animationActive: boolean;
  snapshotDirty: boolean;
}

interface GridSceneRendererOptions {
  onSample?: (sample: GridScenePerfSample) => void;
}

interface FrameResult {
  totalMs?: number;
  dirty?: boolean;
  shouldContinue: boolean;
  stats?: FrameStats;
}

interface FrameStats {
  tileCreates: number;
  tileRemovals: number;
  maskUpdates: number;
  chromeUpdates: number;
  spriteUpdates: number;
  alphaUpdates: number;
  visibleTiles: number;
  activationTiles: number;
  readyTextures: number;
  fadingTiles: number;
}

interface RenderSampleStats {
  renderCount: number;
  renderMsTotal: number;
  renderMsMax: number;
  workMsTotal: number;
  workMsMax: number;
  visibleTilesTotal: number;
  visibleTilesMax: number;
  activationTilesTotal: number;
  activationTilesMax: number;
  readyTexturesTotal: number;
  readyTexturesMax: number;
  fadingTilesTotal: number;
  fadingTilesMax: number;
}

function colorToHex(color: string | null | undefined, fallback: number): number {
  if (!color) return fallback;
  const normalized = color.trim().replace(/^#/, '');
  const canonical = normalized.length === 3
    ? normalized.split('').map((char) => char + char).join('')
    : normalized.length === 8
      ? normalized.slice(0, 6)
      : normalized;
  const parsed = Number.parseInt(canonical, 16);
  if (Number.isFinite(parsed)) return parsed;
  return fallback;
}

function logPerf(label: string, payload: Record<string, unknown>): void {
  console.warn(`[grid-perf-json] ${label} ${JSON.stringify(payload)}`);
}

function createBadge(): BadgeView {
  const background = new Graphics();
  const text = new Text({ text: '', style: badgeTextStyle });
  const container = new Container();
  text.alpha = 0.9;
  container.addChild(background, text);
  container.visible = false;
  return { container, background, text };
}

function updateBadge(
  badge: BadgeView,
  value: string | null,
  x: number,
  y: number,
  align: 'left' | 'right' = 'right',
): number {
  if (!value) {
    badge.container.visible = false;
    return 0;
  }

  badge.container.visible = true;
  badge.text.text = value;
  const width = Math.ceil(badge.text.width) + 10;
  const bx = align === 'right' ? x - width : x;

  badge.background.clear()
    .roundRect(bx, y, width, 18, 4)
    .fill({ color: 0x000000, alpha: 0.55 });
  badge.text.x = bx + 5;
  badge.text.y = Math.round(y + 9 - (badge.text.height / 2));
  return width;
}

function truncateTextToWidth(textNode: Text, text: string, maxWidth: number): string {
  if (!text) return '';
  textNode.text = text;
  if (textNode.width <= maxWidth) return text;

  let lo = 0;
  let hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    const candidate = mid > 0 ? `${text.slice(0, mid)}…` : '…';
    textNode.text = candidate;
    if (textNode.width <= maxWidth) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }

  return lo > 0 ? `${text.slice(0, lo)}…` : '…';
}

function createTileView(): TileView {
  const container = new Container();
  const mediaContainer = new Container();
  const maskSprite = new Sprite(Texture.EMPTY);
  const outerPlaceholder = new Graphics();
  const innerPlaceholder = new Graphics();
  const sprite = new Sprite(Texture.EMPTY);
  const frame = new Graphics();
  const hover = new Graphics();
  const durationBadge = createBadge();
  const collectionBadge = createBadge();
  const extensionBadge = createBadge();
  const indexBadge = createBadge();
  const ratingText = new Text({ text: '', style: ratingTextStyle });
  const nameText = new Text({ text: '', style: nameTextStyle });
  nameText.alpha = 0.85;

  mediaContainer.mask = maskSprite;
  mediaContainer.addChild(sprite);
  container.addChild(outerPlaceholder);
  container.addChild(innerPlaceholder);
  container.addChild(maskSprite);
  container.addChild(mediaContainer);
  container.addChild(frame);
  container.addChild(hover);
  container.addChild(indexBadge.container);
  container.addChild(durationBadge.container);
  container.addChild(collectionBadge.container);
  container.addChild(extensionBadge.container);
  container.addChild(ratingText);
  container.addChild(nameText);
  return {
    container,
    mediaContainer,
    maskSprite,
    frame,
    outerPlaceholder,
    innerPlaceholder,
    hover,
    sprite,
    durationBadge,
    collectionBadge,
    extensionBadge,
    indexBadge,
    ratingText,
    nameText,
    boundIndex: null,
    revealKey: '',
    chromeKey: '',
    spriteKey: '',
    maskKey: '',
    hoverKey: '',
  };
}

export class GridSceneRenderer {
  private readonly app: Application;
  private readonly originalRendererRender: (...args: any[]) => any;
  private readonly stageRoot = new Container();
  private readonly tileLayer = new Container();
  private readonly scrollbarLayer = new Container();
  private readonly scrollbarTrack = new Graphics();
  private readonly scrollbarThumb = new Graphics();
  private readonly tilePool: TileView[] = [];
  private readonly activeTiles = new Map<number, TileView>();
  private snapshot: GridSceneSnapshot | null = null;
  private snapshotDirty = false;
  private animationActive = false;
  private averageFrameMs = 1000 / 60;
  private scrollbarKey = '';
  private readonly revealStateByTile = new Map<string, RevealState>();
  private readonly roundedMaskTextureCache = new Map<string, Texture>();
  private textureStore: GridTextureStore | null = null;
  private readonly onSample?: (sample: GridScenePerfSample) => void;
  private sampleStartedAt = 0;
  private sampleTicks = 0;
  private sampleWorkFrames = 0;
  private renderSample: RenderSampleStats = this.createRenderSampleStats();
  private lastFrameStats: FrameStats | null = null;
  private lastFrameTotalMs = 0;
  constructor(app: Application, options: GridSceneRendererOptions = {}) {
    this.app = app;
    this.originalRendererRender = app.renderer.render.bind(app.renderer);
    this.onSample = options.onSample;
    app.renderer.render = this.handleRender;
    this.stageRoot.addChild(this.tileLayer);
    this.stageRoot.addChild(this.scrollbarLayer);
    this.scrollbarLayer.addChild(this.scrollbarTrack);
    this.scrollbarLayer.addChild(this.scrollbarThumb);
    this.app.stage.addChild(this.stageRoot);
    this.app.ticker.add(this.handleTick);
  }

  setSnapshot(snapshot: GridSceneSnapshot): void {
    this.snapshot = snapshot;
    this.snapshotDirty = true;
  }

  setTextureStore(store: GridTextureStore | null): void {
    this.textureStore = store;
  }

  markTexturesDirty(): void {
    this.animationActive = true;
  }

  destroy(): void {
    this.app.ticker.remove(this.handleTick);
    this.app.renderer.render = this.originalRendererRender;
    for (const tile of this.tilePool) {
      tile.container.destroy({ children: true });
    }
    for (const tile of this.activeTiles.values()) {
      tile.container.destroy({ children: true });
    }
    for (const texture of this.roundedMaskTextureCache.values()) {
      texture.destroy(true);
    }
    this.roundedMaskTextureCache.clear();
    this.stageRoot.destroy({ children: true });
  }

  private getRoundedMaskTexture(width: number, height: number): Texture {
    const safeWidth = Math.max(1, Math.round(width));
    const safeHeight = Math.max(1, Math.round(height));
    const resolution = Math.max(1, Math.round(globalThis.window?.devicePixelRatio ?? 1));
    const key = `${safeWidth}|${safeHeight}|${resolution}|${RADIUS}`;
    const cached = this.roundedMaskTextureCache.get(key);

    if (cached) return cached;

    const canvas = globalThis.document.createElement('canvas');
    canvas.width = safeWidth * resolution;
    canvas.height = safeHeight * resolution;

    const ctx = canvas.getContext('2d');
    if (!ctx) return Texture.EMPTY;

    ctx.scale(resolution, resolution);
    ctx.fillStyle = '#ffffff';
    ctx.beginPath();
    ctx.roundRect(0, 0, safeWidth, safeHeight, RADIUS);
    ctx.fill();

    const texture = Texture.from(canvas);
    this.roundedMaskTextureCache.set(key, texture);

    return texture;
  }

  private handleTick = (): void => {
    const now = performance.now();
    if (this.sampleStartedAt === 0) {
      this.sampleStartedAt = now;
      this.sampleTicks = 0;
      this.sampleWorkFrames = 0;
    }
    this.sampleTicks += 1;
    if (this.snapshotDirty || this.animationActive) {
      this.sampleWorkFrames += 1;
    }
    const sampleElapsed = now - this.sampleStartedAt;
    if (sampleElapsed >= 250) {
      const renderFps = Math.round((this.renderSample.renderCount * 1000) / sampleElapsed);
      const avgRenderMs = this.renderSample.renderCount > 0
        ? this.renderSample.renderMsTotal / this.renderSample.renderCount
        : 0;
      const avgWorkMs = this.sampleWorkFrames > 0
        ? this.renderSample.workMsTotal / this.sampleWorkFrames
        : 0;
      const avgVisibleTiles = this.sampleWorkFrames > 0
        ? this.renderSample.visibleTilesTotal / this.sampleWorkFrames
        : 0;
      const avgActivationTiles = this.sampleWorkFrames > 0
        ? this.renderSample.activationTilesTotal / this.sampleWorkFrames
        : 0;
      const avgReadyTextures = this.sampleWorkFrames > 0
        ? this.renderSample.readyTexturesTotal / this.sampleWorkFrames
        : 0;
      const avgFadingTiles = this.sampleWorkFrames > 0
        ? this.renderSample.fadingTilesTotal / this.sampleWorkFrames
        : 0;
      const sample: GridScenePerfSample = {
        tickerFps: Math.round(this.app.ticker.FPS),
        workFps: Math.round((this.sampleWorkFrames * 1000) / sampleElapsed),
        activeTileCount: this.activeTiles.size,
        snapshotTileCount: this.snapshot?.tiles.length ?? 0,
        animationActive: this.animationActive,
        snapshotDirty: this.snapshotDirty,
      };
      this.onSample?.(sample);
      if (sample.tickerFps < 100 || sample.workFps < 100 || avgRenderMs >= 4) {
        logPerf('pixi-sample', {
          ...sample,
          renderFps,
          avgRenderMs: Number(avgRenderMs.toFixed(2)),
          maxRenderMs: Number(this.renderSample.renderMsMax.toFixed(2)),
          avgWorkMs: Number(avgWorkMs.toFixed(2)),
          maxWorkMs: Number(this.renderSample.workMsMax.toFixed(2)),
          avgVisibleTiles: Number(avgVisibleTiles.toFixed(1)),
          maxVisibleTiles: this.renderSample.visibleTilesMax,
          avgActivationTiles: Number(avgActivationTiles.toFixed(1)),
          maxActivationTiles: this.renderSample.activationTilesMax,
          avgReadyTextures: Number(avgReadyTextures.toFixed(1)),
          maxReadyTextures: this.renderSample.readyTexturesMax,
          avgFadingTiles: Number(avgFadingTiles.toFixed(1)),
          maxFadingTiles: this.renderSample.fadingTilesMax,
        });
      }
      this.sampleStartedAt = now;
      this.sampleTicks = 0;
      this.sampleWorkFrames = 0;
      this.renderSample = this.createRenderSampleStats();
    }

    if (!this.snapshotDirty && !this.animationActive) return;

    const startedAt = now;
    const frameResult = this.renderFrame(startedAt);
    const totalMs = performance.now() - startedAt;
    this.averageFrameMs = this.averageFrameMs * 0.85 + totalMs * 0.15;
    this.snapshotDirty = false;
    this.animationActive = frameResult.shouldContinue;
    this.lastFrameStats = frameResult.stats ?? null;
    this.lastFrameTotalMs = totalMs;
    this.renderSample.workMsTotal += totalMs;
    this.renderSample.workMsMax = Math.max(this.renderSample.workMsMax, totalMs);
    if (frameResult.stats) {
      this.renderSample.visibleTilesTotal += frameResult.stats.visibleTiles;
      this.renderSample.visibleTilesMax = Math.max(this.renderSample.visibleTilesMax, frameResult.stats.visibleTiles);
      this.renderSample.activationTilesTotal += frameResult.stats.activationTiles;
      this.renderSample.activationTilesMax = Math.max(this.renderSample.activationTilesMax, frameResult.stats.activationTiles);
      this.renderSample.readyTexturesTotal += frameResult.stats.readyTextures;
      this.renderSample.readyTexturesMax = Math.max(this.renderSample.readyTexturesMax, frameResult.stats.readyTextures);
      this.renderSample.fadingTilesTotal += frameResult.stats.fadingTiles;
      this.renderSample.fadingTilesMax = Math.max(this.renderSample.fadingTilesMax, frameResult.stats.fadingTiles);
    }

    const slowThresholdMs = Math.max(8, this.averageFrameMs * 1.6);
    if (ENABLE_BENCH_LOGS && totalMs > slowThresholdMs) {
      console.warn('[grid-bench] render-slow', {
        totalMs: Number(totalMs.toFixed(2)),
        avgFrameMs: Number(this.averageFrameMs.toFixed(2)),
        tileCount: this.snapshot?.tiles.length ?? 0,
      });
    }
    if (ENABLE_ACTIVE_FRAME_LOGS && totalMs >= ACTIVE_FRAME_LOG_MS && frameResult.stats) {
      logPerf('active-frame', {
        totalMs: Number(totalMs.toFixed(2)),
        avgFrameMs: Number(this.averageFrameMs.toFixed(2)),
        tileCount: this.snapshot?.tiles.length ?? 0,
        ...frameResult.stats,
      });
    }
  };

  private renderFrame = (now: number): FrameResult => {
    if (!this.snapshot) return { shouldContinue: false };

    const snapshot = this.snapshot;
    const nextActive = new Set<number>();
    let shouldContinue = false;
    const stats: FrameStats = {
      tileCreates: 0,
      tileRemovals: 0,
      maskUpdates: 0,
      chromeUpdates: 0,
      spriteUpdates: 0,
      alphaUpdates: 0,
      visibleTiles: 0,
      activationTiles: 0,
      readyTextures: 0,
      fadingTiles: 0,
    };

    for (let i = 0; i < snapshot.tiles.length; i += 1) {
      const tileSnapshot = snapshot.tiles[i];
      nextActive.add(tileSnapshot.index);
      if (tileSnapshot.isVisible) stats.visibleTiles += 1;
      if (tileSnapshot.isInActivationRange) stats.activationTiles += 1;
      if (tileSnapshot.textureEntry?.status === 'ready' && tileSnapshot.textureEntry.texture) {
        stats.readyTextures += 1;
      }
      let tile = this.activeTiles.get(tileSnapshot.index);
      if (!tile) {
        tile = this.tilePool.pop() ?? createTileView();
        tile.boundIndex = tileSnapshot.index;
        this.activeTiles.set(tileSnapshot.index, tile);
        this.tileLayer.addChild(tile.container);
        stats.tileCreates += 1;
      }
      const tileResult = this.updateTile(tile, tileSnapshot, now, stats);
      shouldContinue = tileResult.shouldContinue || shouldContinue;
    }

    for (const [index, tile] of this.activeTiles) {
      if (nextActive.has(index)) continue;
      if (tile.container.visible) {
        tile.container.visible = false;
      }
      if (tile.container.parent === this.tileLayer) {
        this.tileLayer.removeChild(tile.container);
      }
      if (tile.revealKey) {
        this.revealStateByTile.delete(tile.revealKey);
      }
      if (tile.sprite.visible) {
        tile.sprite.visible = false;
      }
      if (tile.sprite.alpha !== 0) {
        tile.sprite.alpha = 0;
      }
      tile.boundIndex = null;
      tile.revealKey = '';
      tile.chromeKey = '';
      tile.spriteKey = '';
      tile.maskKey = '';
      tile.hoverKey = '';
      this.activeTiles.delete(index);
      this.tilePool.push(tile);
      stats.tileRemovals += 1;
    }

    this.updateScrollbar(snapshot.scrollbar, snapshot.platform);
    return { shouldContinue, stats };
  };

  private handleRender = (...args: any[]): any => {
    const startedAt = performance.now();
    const result = this.originalRendererRender(...args);
    const totalMs = performance.now() - startedAt;
    this.renderSample.renderCount += 1;
    this.renderSample.renderMsTotal += totalMs;
    this.renderSample.renderMsMax = Math.max(this.renderSample.renderMsMax, totalMs);

    if (ENABLE_ACTIVE_FRAME_LOGS && totalMs >= ACTIVE_FRAME_LOG_MS) {
      logPerf('render-pass', {
        totalMs: Number(totalMs.toFixed(2)),
        workMs: Number(this.lastFrameTotalMs.toFixed(2)),
        visibleTiles: this.lastFrameStats?.visibleTiles ?? 0,
        activationTiles: this.lastFrameStats?.activationTiles ?? 0,
        readyTextures: this.lastFrameStats?.readyTextures ?? 0,
        fadingTiles: this.lastFrameStats?.fadingTiles ?? 0,
        activeTileCount: this.activeTiles.size,
        snapshotTileCount: this.snapshot?.tiles.length ?? 0,
      });
    }

    return result;
  };

  private createRenderSampleStats(): RenderSampleStats {
    return {
      renderCount: 0,
      renderMsTotal: 0,
      renderMsMax: 0,
      workMsTotal: 0,
      workMsMax: 0,
      visibleTilesTotal: 0,
      visibleTilesMax: 0,
      activationTilesTotal: 0,
      activationTilesMax: 0,
      readyTexturesTotal: 0,
      readyTexturesMax: 0,
      fadingTilesTotal: 0,
      fadingTilesMax: 0,
    };
  }

  private updateTile(tile: TileView, snapshot: SceneTileSnapshot, now: number, stats: FrameStats): FrameResult {
    const { position, renderItem, showName, showExtension, viewMode } = snapshot;
    const textureEntry = snapshot.textureEntry ?? this.textureStore?.get(snapshot.thumbnailHash) ?? null;
    const imageHeight = Math.max(0, position.h - (showName ? 20 : 0));
    const useContain = viewMode === 'grid' || renderItem.mime.startsWith('video/');
    const placeholderColor = renderItem.dominantColor
      ? colorToHex(renderItem.dominantColor, PLACEHOLDER_BG_COLOR)
      : PLACEHOLDER_BG_COLOR;
    const containRect = useContain
      ? getContainRect(renderItem.aspectRatio ?? 1, 0, 0, position.w, imageHeight)
      : null;
    const clipRect = containRect ?? { x: 0, y: 0, w: position.w, h: imageHeight };

    let dirty = false;
    if (!tile.container.visible) {
      tile.container.visible = true;
      dirty = true;
    }
    if (tile.container.x !== position.x) {
      tile.container.x = position.x;
      dirty = true;
    }
    if (tile.container.y !== position.y) {
      tile.container.y = position.y;
      dirty = true;
    }
    if (tile.mediaContainer.x !== 0) {
      tile.mediaContainer.x = 0;
      dirty = true;
    }
    if (tile.mediaContainer.y !== 0) {
      tile.mediaContainer.y = 0;
      dirty = true;
    }
    if (!tile.maskSprite.visible) {
      tile.maskSprite.visible = true;
      dirty = true;
    }

    let revealAlpha = 0;
    let shouldContinue = false;
    const revealKey = `${snapshot.index}:${snapshot.thumbnailHash}`;
    tile.revealKey = revealKey;
    const ready = textureEntry?.status === 'ready' && !!textureEntry.texture;

    if (snapshot.suppressTileReveal) {
      this.revealStateByTile.delete(revealKey);
      revealAlpha = ready ? 1 : 0;
    } else if (!snapshot.isInActivationRange) {
      this.revealStateByTile.delete(revealKey);
      revealAlpha = 0;
    } else {
      let revealState = this.revealStateByTile.get(revealKey);
      if (!revealState) {
        revealState = {
          status: 'activation_pending',
          activationSince: now,
          fadeStartedAt: null,
        };
        this.revealStateByTile.set(revealKey, revealState);
      }

      const dwellMs = now - revealState.activationSince;
      if (dwellMs < ACTIVATION_DWELL_MS) {
        revealAlpha = 0;
        shouldContinue = snapshot.isVisible;
      } else {
        if (revealState.status === 'activation_pending') {
          revealState.status = 'activation_ready';
        }

        if (!ready) {
          revealAlpha = 0;
        } else if (!snapshot.isVisible) {
          revealAlpha = revealState.status === 'shown' ? 1 : 0;
        } else if (revealState.status === 'shown') {
          revealAlpha = 1;
        } else if (revealState.status === 'fading' && revealState.fadeStartedAt != null) {
          stats.fadingTiles += 1;
          const progress = Math.max(0, Math.min(1, (now - revealState.fadeStartedAt) / REVEAL_FADE_MS));
          revealAlpha = progress;
          if (progress < 1) {
            shouldContinue = true;
          } else {
            revealState.status = 'shown';
            revealState.fadeStartedAt = null;
            revealAlpha = 1;
          }
        } else {
          revealState.status = 'fading';
          revealState.fadeStartedAt = now;
          revealAlpha = 0;
          shouldContinue = true;
        }
      }
    }

    const containRectKey = containRect
      ? `${containRect.x.toFixed(1)}|${containRect.y.toFixed(1)}|${containRect.w.toFixed(1)}|${containRect.h.toFixed(1)}`
      : 'none';
    const maskKey = `${clipRect.x.toFixed(1)}|${clipRect.y.toFixed(1)}|${clipRect.w.toFixed(1)}|${clipRect.h.toFixed(1)}`;
    const chromeKey = [
      position.w.toFixed(1),
      imageHeight.toFixed(1),
      useContain ? '1' : '0',
      placeholderColor,
      containRectKey,
      showName ? '1' : '0',
      showExtension ? '1' : '0',
      renderItem.durationMs ?? '',
      renderItem.kind,
      renderItem.memberCount ?? '',
      renderItem.rating ?? '',
      renderItem.name ?? '',
      renderItem.mime,
    ].join('|');

    if (tile.maskKey !== maskKey) {
      tile.maskKey = maskKey;
      dirty = true;
      stats.maskUpdates += 1;
      tile.maskSprite.texture = this.getRoundedMaskTexture(clipRect.w, clipRect.h);
      tile.maskSprite.x = clipRect.x;
      tile.maskSprite.y = clipRect.y;
      tile.maskSprite.width = clipRect.w;
      tile.maskSprite.height = clipRect.h;
    }

    if (tile.chromeKey !== chromeKey) {
      tile.chromeKey = chromeKey;
      dirty = true;
      stats.chromeUpdates += 1;

      tile.frame.clear();

      tile.outerPlaceholder.clear();
      tile.innerPlaceholder.clear();

      if (useContain && containRect) {
        tile.frame.roundRect(
          containRect.x + 0.5,
          containRect.y + 0.5,
          Math.max(0, containRect.w - 1),
          Math.max(0, containRect.h - 1),
          RADIUS,
        ).stroke({ color: BORDER_COLOR, alpha: 0.2, width: 1 });
        tile.outerPlaceholder
          .roundRect(0, 0, position.w, imageHeight, RADIUS)
          .fill({ color: PLACEHOLDER_BG_COLOR, alpha: PLACEHOLDER_BG_ALPHA });
        if (renderItem.dominantColor) {
          tile.innerPlaceholder.roundRect(
            containRect.x,
            containRect.y,
            containRect.w,
            containRect.h,
            RADIUS,
          ).fill({ color: placeholderColor, alpha: 1 });
        }
      } else {
        tile.frame.roundRect(0.5, 0.5, Math.max(0, position.w - 1), Math.max(0, imageHeight - 1), RADIUS)
          .stroke({ color: BORDER_COLOR, alpha: 0.2, width: 1 });
        tile.outerPlaceholder
          .roundRect(0, 0, position.w, imageHeight, RADIUS)
          .fill({ color: placeholderColor, alpha: renderItem.dominantColor ? 1 : PLACEHOLDER_BG_ALPHA });
      }

      const extension = showExtension ? mimeToExt(renderItem.mime) : '';
      const showExtensionBadge = !!extension && !isHiddenBadgeType(extension);
      const durationWidth = updateBadge(
        tile.durationBadge,
        renderItem.durationMs != null ? formatDuration(renderItem.durationMs) : null,
        position.w - 4,
        4,
      );
      updateBadge(
        tile.collectionBadge,
        renderItem.kind === 'collection' && renderItem.memberCount != null ? String(renderItem.memberCount) : null,
        position.w - 4 - durationWidth - (durationWidth > 0 ? BADGE_GAP : 0),
        4,
      );
      updateBadge(tile.extensionBadge, showExtensionBadge ? extension : null, position.w - 4, imageHeight - 22);
      updateBadge(tile.indexBadge, null, 4, 4, 'left');

      tile.ratingText.visible = !!renderItem.rating && renderItem.rating > 0;
      tile.ratingText.text = tile.ratingText.visible ? '★'.repeat(renderItem.rating ?? 0) : '';
      tile.ratingText.x = 5;
      tile.ratingText.y = 5;

      tile.nameText.visible = showName;
      tile.nameText.x = 2;
      tile.nameText.y = imageHeight + 3;
      tile.nameText.text = showName
        ? truncateTextToWidth(tile.nameText, renderItem.name ?? '', Math.max(0, position.w - 4))
        : '';
    }

    const spriteKey = textureEntry?.status === 'ready' && textureEntry.texture
      ? [
          snapshot.thumbnailHash,
          position.w.toFixed(1),
          imageHeight.toFixed(1),
          useContain ? '1' : '0',
          textureEntry.texture.width || 1,
          textureEntry.texture.height || 1,
        ].join('|')
      : 'none';

    if (tile.spriteKey !== spriteKey) {
      tile.spriteKey = spriteKey;
      dirty = true;
      stats.spriteUpdates += 1;
      if (textureEntry?.status === 'ready' && textureEntry.texture) {
        tile.sprite.visible = true;
        tile.sprite.texture = textureEntry.texture;
        const tw = textureEntry.texture.width || 1;
        const th = textureEntry.texture.height || 1;
        const scale = useContain
          ? Math.min(position.w / tw, imageHeight / th)
          : Math.max(position.w / tw, imageHeight / th);
        const width = tw * scale;
        const height = th * scale;
        tile.sprite.width = width;
        tile.sprite.height = height;
        tile.sprite.x = (position.w - width) / 2;
        tile.sprite.y = (imageHeight - height) / 2;
      } else {
        tile.sprite.visible = false;
        tile.sprite.alpha = 0;
      }
    }

    if (tile.sprite.alpha !== revealAlpha) {
      tile.sprite.alpha = revealAlpha;
      dirty = true;
      stats.alphaUpdates += 1;
    }

    if (tile.outerPlaceholder.alpha !== 1) {
      tile.outerPlaceholder.alpha = 1;
      dirty = true;
    }
    const innerAlpha = useContain ? 1 : 0;
    if (tile.innerPlaceholder.alpha !== innerAlpha) {
      tile.innerPlaceholder.alpha = innerAlpha;
      dirty = true;
    }

    const hoverKey = `${position.w.toFixed(1)}|${imageHeight.toFixed(1)}`;
    if (tile.hoverKey !== hoverKey) {
      tile.hoverKey = hoverKey;
      dirty = true;
      tile.hover.clear();
    }

    return { dirty, shouldContinue };
  }

  private updateScrollbar(scrollbar: ScrollbarVisualState, platform: ScrollPlatformProfile): boolean {
    let dirty = false;
    const nextVisible = scrollbar.opacity > 0;
    if (this.scrollbarLayer.visible !== nextVisible) {
      this.scrollbarLayer.visible = nextVisible;
      dirty = true;
    }
    if (this.scrollbarLayer.alpha !== scrollbar.opacity) {
      this.scrollbarLayer.alpha = scrollbar.opacity;
      dirty = true;
    }
    const scrollbarKey = [
      platform,
      scrollbar.trackX.toFixed(1),
      scrollbar.trackY.toFixed(1),
      scrollbar.trackWidth.toFixed(1),
      scrollbar.trackHeight.toFixed(1),
      scrollbar.thumbY.toFixed(1),
      scrollbar.thumbHeight.toFixed(1),
      scrollbar.opacity.toFixed(2),
      scrollbar.showTrack ? '1' : '0',
    ].join('|');
    if (this.scrollbarKey === scrollbarKey) return dirty;
    this.scrollbarKey = scrollbarKey;
    dirty = true;
    this.scrollbarTrack.clear();
    this.scrollbarThumb.clear();

    const thumbInset = 2;
    const thumbWidth = Math.max(4, scrollbar.trackWidth - thumbInset * 2);
    const thumbX = scrollbar.trackX + thumbInset;
    const thumbRadius = Math.min(4, thumbWidth / 2);

    this.scrollbarThumb
      .roundRect(thumbX, scrollbar.thumbY, thumbWidth, scrollbar.thumbHeight, thumbRadius)
      .fill({ color: 0xffffff, alpha: 0.12 });
    return dirty;
  }
}
