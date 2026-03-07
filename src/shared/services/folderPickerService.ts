// PBI-037: Imperative service singleton — no React exports.
import type { FilterLogicMode } from '../../state/filterStore';

export interface FolderPickerRequest {
  anchorEl: HTMLElement;
  /** Optional explicit screen-space anchor (used by context menu actions). */
  anchorPoint?: { x: number; y: number };
  selectedFolderIds: number[];
  excludedFolderIds?: number[];
  logicMode?: FilterLogicMode;
  onToggle: (folderId: number, folderName: string, added: boolean) => void;
  /** When provided, enables filter mode: right-click exclude, logic tabs, footer tips. */
  onExclude?: (folderId: number, folderName: string) => void;
  onLogicChange?: (mode: FilterLogicMode) => void;
}

let _openFn: ((req: FolderPickerRequest) => void) | null = null;

export const FolderPickerService = {
  open(opts: FolderPickerRequest) {
    _openFn?.(opts);
  },
};

export function registerFolderPickerOpenHandler(
  handler: (req: FolderPickerRequest) => void,
): () => void {
  _openFn = handler;
  return () => {
    if (_openFn === handler) _openFn = null;
  };
}
