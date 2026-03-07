import { SelectionController } from '../../controllers/selectionController';
import { SidebarController } from '../../controllers/sidebarController';
import { useCacheStore } from '../../state/cacheStore';
import { useDomainStore } from '../../state/domainStore';

export interface MutationEffects {
  selectionSummary?: boolean;
  sidebarTree?: boolean;
  gridCaches?: boolean;
  gridReload?: (() => void) | null;
}

export const LIFECYCLE_MUTATION_EFFECTS: MutationEffects = {
  selectionSummary: true,
  sidebarTree: true,
  gridCaches: true,
  gridReload: null,
};

export const GRID_ONLY_MUTATION_EFFECTS: MutationEffects = {
  selectionSummary: false,
  sidebarTree: false,
  gridCaches: true,
  gridReload: null,
};

/**
 * Apply frontend mutation side effects from one place so components do not
 * duplicate sidebar/cache refresh fanout logic.
 */
export function applyMutationEffects(effects: MutationEffects = LIFECYCLE_MUTATION_EFFECTS): void {
  if (effects.selectionSummary) {
    SelectionController.invalidateSummary();
  }
  if (effects.sidebarTree) {
    void useDomainStore.getState().fetchSidebarTree();
    SidebarController.requestRefresh();
  }
  if (effects.gridCaches) {
    useCacheStore.getState().invalidateAll();
    useCacheStore.getState().bumpGridRefresh();
  }
  effects.gridReload?.();
}

export function applyLifecycleMutationEffects(gridReload?: () => void): void {
  applyMutationEffects({
    ...LIFECYCLE_MUTATION_EFFECTS,
    gridReload: gridReload ?? null,
  });
}

export function applyGridMutationEffects(gridReload?: () => void): void {
  applyMutationEffects({
    ...GRID_ONLY_MUTATION_EFFECTS,
    gridReload: gridReload ?? null,
  });
}
