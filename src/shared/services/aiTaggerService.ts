/** PBI-525: Imperative service singleton for AI tagger portal — no React exports. */

export interface AiTaggerRequest {
  anchorEl: HTMLElement;
  hashes: string[];
  onApply: (tags: string[]) => Promise<void>;
}

type OpenHandler = (req: AiTaggerRequest) => void;
let _openFn: OpenHandler | null = null;

export function registerAiTaggerOpenHandler(handler: OpenHandler): () => void {
  _openFn = handler;
  return () => { _openFn = null; };
}

export const AiTaggerService = {
  open(request: AiTaggerRequest) {
    _openFn?.(request);
  },
};
