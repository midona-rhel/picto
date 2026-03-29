import { useCallback, useEffect, useRef, useState } from 'react';
import { DEFAULT_FPS } from './videoConstants';

function snapFps(measured: number): number {
  const common = [23.976, 24, 25, 29.97, 30, 48, 50, 59.94, 60, 120];
  let best = DEFAULT_FPS;
  let bestDist = Infinity;
  for (const c of common) {
    const d = Math.abs(measured - c);
    if (d < bestDist) { bestDist = d; best = c; }
  }
  return bestDist / best < 0.1 ? best : Math.round(measured);
}

export function useFrameTime(videoRef: React.RefObject<HTMLVideoElement | null>) {
  const [fps, setFps] = useState(DEFAULT_FPS);
  const [frameTime, setFrameTime] = useState(0);
  const handleRef = useRef(0);
  const prevMediaTimeRef = useRef(-1);
  const prevFramesRef = useRef(-1);
  const accRef = useRef<number[]>([]);

  const tick = useCallback((_now: DOMHighResTimeStamp, meta: VideoFrameCallbackMetadata) => {
    setFrameTime(meta.mediaTime);
    if (prevMediaTimeRef.current >= 0 && prevFramesRef.current >= 0) {
      const fd = meta.presentedFrames - prevFramesRef.current;
      const td = meta.mediaTime - prevMediaTimeRef.current;
      if (fd > 0 && td > 0) {
        accRef.current.push(fd / td);
        if (accRef.current.length >= 10) {
          accRef.current.sort((a, b) => a - b);
          setFps(snapFps(accRef.current[Math.floor(accRef.current.length / 2)]));
          accRef.current = accRef.current.slice(-5);
        }
      }
    }
    prevMediaTimeRef.current = meta.mediaTime;
    prevFramesRef.current = meta.presentedFrames;
    const video = videoRef.current;
    if (video && 'requestVideoFrameCallback' in video) {
      handleRef.current = video.requestVideoFrameCallback(tick);
    }
  }, [videoRef]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video || !('requestVideoFrameCallback' in video)) return;
    handleRef.current = video.requestVideoFrameCallback(tick);
    return () => { if (handleRef.current) video.cancelVideoFrameCallback(handleRef.current); };
  }, [videoRef, tick]);

  useEffect(() => {
    prevMediaTimeRef.current = -1;
    prevFramesRef.current = -1;
    accRef.current = [];
    setFps(DEFAULT_FPS);
    setFrameTime(0);
  }, [videoRef.current?.src]); // eslint-disable-line react-hooks/exhaustive-deps

  return { frameTime, fps };
}
