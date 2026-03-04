import type { TagSelectPanelProps } from './tagSelectTypes';

let openHandler: ((props: TagSelectPanelProps) => void) | null = null;

export const TagSelectService = {
  open(opts: TagSelectPanelProps) {
    openHandler?.(opts);
  },
};

export function registerTagSelectOpenHandler(
  handler: (props: TagSelectPanelProps) => void,
): () => void {
  openHandler = handler;
  return () => {
    if (openHandler === handler) openHandler = null;
  };
}
