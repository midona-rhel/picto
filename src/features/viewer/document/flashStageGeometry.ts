export interface FlashStageSize {
  width: number;
  height: number;
}

export function fitFlashStage(viewport: FlashStageSize, movie: FlashStageSize | null): FlashStageSize | null {
  if (!(viewport.width > 0) || !(viewport.height > 0)) return null;
  if (!movie || !(movie.width > 0) || !(movie.height > 0)) return viewport;
  const scale = Math.min(viewport.width / movie.width, viewport.height / movie.height);
  return {
    width: movie.width * scale,
    height: movie.height * scale,
  };
}
