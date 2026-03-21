import type { SelectionQuerySpec } from '../metadataPrefetch';

export type VirtualAllSelectionState = {
  baseSpec: Omit<SelectionQuerySpec, 'excluded_hashes'>;
  excludedHashes: Set<string>;
};

export interface GridUiState {
  selectedHashes: Set<string>;
  virtualAllSelection: VirtualAllSelectionState | null;
  virtualAllSelectedCount: number | null;
  lastClickedHash: string | null;
  selectedSubfolderId: number | null;
  popHash: string | null;
  boxActive: boolean;
  isDragOver: boolean;
}

export function createInitialGridUiState(): GridUiState {
  return {
    selectedHashes: new Set(),
    virtualAllSelection: null,
    virtualAllSelectedCount: null,
    lastClickedHash: null,
    selectedSubfolderId: null,
    popHash: null,
    boxActive: false,
    isDragOver: false,
  };
}
