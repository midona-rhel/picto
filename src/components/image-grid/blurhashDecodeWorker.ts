import { decode } from 'blurhash';

type WorkerCtx = {
  onmessage: ((event: MessageEvent<DecodeRequest>) => void) | null;
  postMessage: (message: DecodeResponse, transfer?: Transferable[]) => void;
};

const ctx = self as unknown as WorkerCtx;

interface DecodeRequest {
  id: number;
  hash: string;
  blurhash: string;
  aspectRatio: number;
}

interface DecodeResponse {
  id: number;
  hash: string;
  width: number;
  height: number;
  pixels: ArrayBuffer;
}

function resolveDimensions(aspectRatio: number): { width: number; height: number } {
  if (aspectRatio >= 1) {
    const width = 32;
    const height = Math.max(1, Math.round(32 / aspectRatio));
    return { width, height };
  }
  const height = 32;
  const width = Math.max(1, Math.round(32 * aspectRatio));
  return { width, height };
}

ctx.onmessage = (event: MessageEvent<DecodeRequest>) => {
  const msg = event.data;
  try {
    const { width, height } = resolveDimensions(msg.aspectRatio);
    const rgba = decode(msg.blurhash, width, height);
    const typed = new Uint8ClampedArray(rgba);
    const response: DecodeResponse = {
      id: msg.id,
      hash: msg.hash,
      width,
      height,
      pixels: typed.buffer,
    };
    ctx.postMessage(response, [typed.buffer]);
  } catch {
    // Ignore invalid blurhash input; atlas will keep placeholder fallback.
  }
};
