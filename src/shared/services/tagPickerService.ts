// PBI-037: Imperative service singleton — no React exports.

export interface TagPickerRequest {
  anchorEl: HTMLElement;
  selected: string[];
  onToggle: (tag: string, added: boolean) => void;
}

let _openFn: ((req: TagPickerRequest) => void) | null = null;

export const TagPickerService = {
  open(opts: TagPickerRequest) {
    _openFn?.(opts);
  },
};

export function registerTagPickerOpenHandler(
  handler: (req: TagPickerRequest) => void,
): () => void {
  _openFn = handler;
  return () => {
    if (_openFn === handler) _openFn = null;
  };
}
