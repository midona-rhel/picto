/**
 * useFrameTime — frame-accurate current time and FPS detection
 * via requestVideoFrameCallback (Chromium/Electron).
 *
 * Uses the built-in DOM types for VideoFrameCallbackMetadata.
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { DEFAULT_FPS } from './videoConstants';

export interface FrameTimeResult {
  /** Frame-accurate current time (seconds). Falls back to video.currentTime. */
  frameTime: number;
  /** Detected FPS, or DEFAULT_FPS if detection failed. */
  fps: number;
  /** Whether requestVideoFrameCallback is available. */
  isFrameCallbackSupported: boolean;
}

export function useFrameTime(videoRef: React.RefObject<HTMLVideoElement | null>): FrameTimeResult {
  const [fps, setFps] = useState(DEFAULT_FPS);
  const [frameTime, setFrameTime] = useState(0);
  const [supported, setSupported] = useState(false);

  const handleRef = useRef(0);
  const prevMediaTimeRef = useRef(-1);
  const prevFramesRef = useRef(-1);
  const fpsAccumulatorRef = useRef<number[]>([]);

  const tick = useCallback((_now: DOMHighResTimeStamp, metadata: VideoFrameCallbackMetadata) => {
    setFrameTime(metadata.mediaTime);

    // FPS detection: accumulate frame deltas
    if (prevMediaTimeRef.current >= 0 && prevFramesRef.current >= 0) {
      const frameDelta = metadata.presentedFrames - prevFramesRef.current;
      const timeDelta = metadata.mediaTime - prevMediaTimeRef.current;
      if (frameDelta > 0 && timeDelta > 0) {
        const instantFps = frameDelta / timeDelta;
        const acc = fpsAccumulatorRef.current;
        acc.push(instantFps);
        // Stabilize after collecting enough samples
        if (acc.length >= 10) {
          acc.sort((a, b) => a - b);
          // Use median for robustness
          const median = acc[Math.floor(acc.length / 2)];
          // Snap to common frame rates
          const snapped = snapFps(median);
          setFps(snapped);
          // Reset accumulator but keep collecting (in case rate changes)
          fpsAccumulatorRef.current = acc.slice(-5);
        }
      }
    }
    prevMediaTimeRef.current = metadata.mediaTime;
    prevFramesRef.current = metadata.presentedFrames;

    const video = videoRef.current;
    if (video && 'requestVideoFrameCallback' in video) {
      handleRef.current = video.requestVideoFrameCallback(tick);
    }
  }, [videoRef]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // requestVideoFrameCallback is always available in Chromium/Electron
    setSupported(true);
    handleRef.current = video.requestVideoFrameCallback(tick);
    return () => {
      if (handleRef.current) {
        video.cancelVideoFrameCallback(handleRef.current);
      }
    };
  }, [videoRef, tick]);

  // Reset when video src changes
  useEffect(() => {
    prevMediaTimeRef.current = -1;
    prevFramesRef.current = -1;
    fpsAccumulatorRef.current = [];
    setFps(DEFAULT_FPS);
    setFrameTime(0);
  }, [videoRef.current?.src]); // eslint-disable-line react-hooks/exhaustive-deps

  return { frameTime, fps, isFrameCallbackSupported: supported };
}

/** Snap a measured FPS to the nearest common frame rate. */
function snapFps(measured: number): number {
  const common = [23.976, 24, 25, 29.97, 30, 48, 50, 59.94, 60, 120];
  let best = DEFAULT_FPS;
  let bestDist = Infinity;
  for (const c of common) {
    const d = Math.abs(measured - c);
    if (d < bestDist) {
      bestDist = d;
      best = c;
    }
  }
  // Only snap if within 10% tolerance
  return bestDist / best < 0.1 ? best : Math.round(measured);
}
