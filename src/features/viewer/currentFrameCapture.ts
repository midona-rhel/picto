export type CurrentFrameCapture = () => Promise<string>;

export function captureVideoFrame(video: HTMLVideoElement): string {
  if (!(video.videoWidth > 0) || !(video.videoHeight > 0)) {
    throw new Error('The video frame is not ready yet.');
  }
  const canvas = document.createElement('canvas');
  canvas.width = video.videoWidth;
  canvas.height = video.videoHeight;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('Could not create a thumbnail canvas.');
  context.drawImage(video, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL('image/png');
}
