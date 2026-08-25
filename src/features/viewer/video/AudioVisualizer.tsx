import { useEffect, useRef, type RefObject } from 'react';
import type { AudioVisualizationMode } from '../../../shared/lib/audioVisualization';
import styles from './AudioVisualizer.module.css';

// Original Canvas renderer over the standard Web Audio analyser. No third-party
// visualizer code or preset assets are bundled.
interface Props {
  mediaRef: RefObject<HTMLMediaElement | null>;
  mode: AudioVisualizationMode;
}

const BAR_COUNT = 64;

function frequencyBin(data: Uint8Array<ArrayBuffer>, index: number): number {
  const normalized = index / (BAR_COUNT - 1);
  return data[Math.min(data.length - 1, Math.floor(normalized ** 1.7 * data.length))] / 255;
}

function drawStage(ctx: CanvasRenderingContext2D, width: number, height: number) {
  const sky = ctx.createLinearGradient(0, 0, 0, height);
  sky.addColorStop(0, '#05050b');
  sky.addColorStop(0.58, '#0d0920');
  sky.addColorStop(1, '#04040a');
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, width, height);

  const atmosphere = ctx.createRadialGradient(
    width / 2, height * 0.54, 0,
    width / 2, height * 0.54, Math.max(width * 0.58, height),
  );
  atmosphere.addColorStop(0, 'rgba(115, 92, 255, 0.18)');
  atmosphere.addColorStop(0.34, 'rgba(255, 79, 163, 0.08)');
  atmosphere.addColorStop(1, 'rgba(5, 5, 11, 0)');
  ctx.fillStyle = atmosphere;
  ctx.fillRect(0, 0, width, height);

}

function drawSpectrum(ctx: CanvasRenderingContext2D, data: Uint8Array<ArrayBuffer>, width: number, height: number) {
  const gap = Math.max(2, width * 0.003);
  const usableWidth = width * 0.96;
  const barWidth = Math.max(2, (usableWidth - gap * (BAR_COUNT - 1)) / BAR_COUNT);
  const left = (width - usableWidth) / 2;
  const middle = height * 0.42;
  const amplitude = height * 0.25;
  const gradient = ctx.createLinearGradient(0, middle - amplitude, width, middle + amplitude);
  gradient.addColorStop(0, '#31d7ff');
  gradient.addColorStop(0.48, '#735cff');
  gradient.addColorStop(1, '#ff4fa3');
  ctx.fillStyle = gradient;
  ctx.beginPath();

  for (let index = 0; index < BAR_COUNT; index += 1) {
    const value = Math.max(0.025, frequencyBin(data, index));
    const barHeight = Math.max(2, value * amplitude);
    const x = left + index * (barWidth + gap);
    ctx.roundRect(x, middle - barHeight, barWidth, barHeight * 2, barWidth / 2);
  }
  ctx.fill();
}

function drawOscilloscope(ctx: CanvasRenderingContext2D, data: Uint8Array<ArrayBuffer>, width: number, height: number) {
  const gradient = ctx.createLinearGradient(width * 0.12, 0, width * 0.88, 0);
  gradient.addColorStop(0, '#31d7ff');
  gradient.addColorStop(0.5, '#735cff');
  gradient.addColorStop(1, '#ff4fa3');
  ctx.strokeStyle = gradient;
  ctx.lineWidth = Math.max(2, Math.min(4, height * 0.012));
  ctx.lineJoin = 'round';
  ctx.lineCap = 'round';
  ctx.beginPath();
  const left = width * 0.02;
  const usableWidth = width * 0.96;
  for (let index = 0; index < data.length; index += 1) {
    const x = left + (index / (data.length - 1)) * usableWidth;
    const y = height * 0.42 + ((data[index] - 128) / 128) * height * 0.25;
    if (index === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
}

function drawOrbit(ctx: CanvasRenderingContext2D, data: Uint8Array<ArrayBuffer>, width: number, height: number) {
  const centerX = width / 2;
  const centerY = height * 0.42;
  const radius = Math.min(width, height) * 0.17;
  const reach = Math.min(width, height) * 0.22;
  const gradient = ctx.createConicGradient(-Math.PI / 2, centerX, centerY);
  gradient.addColorStop(0, '#31d7ff');
  gradient.addColorStop(0.34, '#735cff');
  gradient.addColorStop(0.67, '#ff4fa3');
  gradient.addColorStop(1, '#31d7ff');
  ctx.strokeStyle = gradient;
  ctx.lineWidth = Math.max(2, Math.min(5, radius * 0.055));
  ctx.lineCap = 'round';
  ctx.beginPath();
  for (let index = 0; index < BAR_COUNT; index += 1) {
    const angle = (index / BAR_COUNT) * Math.PI * 2 - Math.PI / 2;
    const value = Math.max(0.035, frequencyBin(data, index));
    const outer = radius + value * reach;
    ctx.moveTo(centerX + Math.cos(angle) * radius, centerY + Math.sin(angle) * radius);
    ctx.lineTo(centerX + Math.cos(angle) * outer, centerY + Math.sin(angle) * outer);
  }
  ctx.stroke();
}

function drawVisualization(
  ctx: CanvasRenderingContext2D,
  mode: Exclude<AudioVisualizationMode, 'none'>,
  data: Uint8Array<ArrayBuffer>,
  width: number,
  height: number,
) {
  if (mode === 'spectrum') drawSpectrum(ctx, data, width, height);
  else if (mode === 'oscilloscope') drawOscilloscope(ctx, data, width, height);
  else drawOrbit(ctx, data, width, height);
}

function drawReflection(
  ctx: CanvasRenderingContext2D,
  mode: Exclude<AudioVisualizationMode, 'none'>,
  data: Uint8Array<ArrayBuffer>,
  width: number,
  height: number,
) {
  // Reflect the exact analyser geometry rather than synthesizing a separate
  // floor animation. Two soft passes feather it without hiding its source.
  for (const [alpha, blur] of [[0.07, 0.03], [0.09, 0.012]] as const) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, height * 0.58, width, height * 0.42);
    ctx.clip();
    ctx.translate(0, height * 0.86);
    ctx.scale(1, -0.34);
    ctx.globalAlpha = alpha;
    ctx.globalCompositeOperation = 'lighter';
    ctx.filter = `blur(${Math.max(8, Math.min(30, height * blur))}px)`;
    drawVisualization(ctx, mode, data, width, height);
    ctx.restore();
  }
}

export function AudioVisualizer({ mediaRef, mode }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const modeRef = useRef(mode);
  const frameRef = useRef(0);
  const drawRef = useRef<() => void>(() => {});
  const audioContextRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const sourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const dataRef = useRef<Uint8Array<ArrayBuffer> | null>(null);

  modeRef.current = mode;

  useEffect(() => {
    const canvas = canvasRef.current;
    const media = mediaRef.current;
    if (!canvas || !media) return;

    let disposed = false;
    const sizeCanvas = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const width = Math.max(1, Math.round(rect.width * dpr));
      const height = Math.max(1, Math.round(rect.height * dpr));
      if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
      }
    };

    const ensureGraph = () => {
      if (analyserRef.current || modeRef.current === 'none') return analyserRef.current;
      if (!window.AudioContext) return null;
      let audioContext: AudioContext | null = null;
      try {
        const captureStream = (media as HTMLMediaElement & {
          captureStream?: () => MediaStream;
        }).captureStream;
        if (!captureStream) return null;
        const stream = captureStream.call(media);
        audioContext = new AudioContext({ latencyHint: 'playback' });
        const source = audioContext.createMediaStreamSource(stream);
        const analyser = audioContext.createAnalyser();
        analyser.fftSize = modeRef.current === 'oscilloscope' ? 1024 : 256;
        analyser.smoothingTimeConstant = 0.78;
        source.connect(analyser);
        audioContextRef.current = audioContext;
        analyserRef.current = analyser;
        sourceRef.current = source;
        if (!media.paused) void audioContext.resume();
        return analyser;
      } catch {
        void audioContext?.close();
        return null;
      }
    };

    const draw = () => {
      frameRef.current = 0;
      if (disposed) return;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;
      sizeCanvas();
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      if (modeRef.current === 'none') return;

      const analyser = ensureGraph();
      if (!analyser) {
        drawStage(ctx, canvas.width, canvas.height);
        if (!media.paused && !media.ended) frameRef.current = requestAnimationFrame(draw);
        return;
      }
      const fftSize = modeRef.current === 'oscilloscope' ? 1024 : 256;
      if (analyser.fftSize !== fftSize) analyser.fftSize = fftSize;
      const dataLength = modeRef.current === 'oscilloscope' ? analyser.fftSize : analyser.frequencyBinCount;
      if (dataRef.current?.length !== dataLength) {
        dataRef.current = new Uint8Array(new ArrayBuffer(dataLength));
      }
      const data = dataRef.current;
      if (modeRef.current === 'oscilloscope') analyser.getByteTimeDomainData(data);
      else analyser.getByteFrequencyData(data);

      drawStage(ctx, canvas.width, canvas.height);
      const activeMode = modeRef.current;
      drawReflection(ctx, activeMode, data, canvas.width, canvas.height);
      drawVisualization(ctx, activeMode, data, canvas.width, canvas.height);

      if (!media.paused && !media.ended) frameRef.current = requestAnimationFrame(draw);
    };
    drawRef.current = draw;

    const start = () => {
      ensureGraph();
      void audioContextRef.current?.resume();
      if (!frameRef.current) frameRef.current = requestAnimationFrame(draw);
    };
    const stop = () => {
      if (frameRef.current) cancelAnimationFrame(frameRef.current);
      frameRef.current = 0;
      draw();
    };
    const observer = typeof ResizeObserver === 'undefined'
      ? null
      : new ResizeObserver(() => {
          sizeCanvas();
          if (!frameRef.current) draw();
        });
    observer?.observe(canvas);
    media.addEventListener('play', start);
    media.addEventListener('pause', stop);
    media.addEventListener('seeked', draw);
    sizeCanvas();
    draw();

    return () => {
      disposed = true;
      observer?.disconnect();
      media.removeEventListener('play', start);
      media.removeEventListener('pause', stop);
      media.removeEventListener('seeked', draw);
      if (frameRef.current) cancelAnimationFrame(frameRef.current);
      sourceRef.current?.disconnect();
      analyserRef.current?.disconnect();
      void audioContextRef.current?.close();
      analyserRef.current = null;
      sourceRef.current = null;
      audioContextRef.current = null;
      dataRef.current = null;
      drawRef.current = () => {};
    };
  }, [mediaRef]);

  useEffect(() => {
    drawRef.current();
  }, [mode]);

  return <canvas ref={canvasRef} className={styles.canvas} data-audio-visualization={mode} aria-hidden />;
}
