import { startApplyingSidebarRefreshTargets, stopApplyingSidebarRefreshTargets } from './stateChanges/applySidebarRefreshTargets';
import { startApplyingGridRefreshTargets, stopApplyingGridRefreshTargets } from './stateChanges/applyGridRefreshTargets';
import { startApplyingSelectionRefreshTargets, stopApplyingSelectionRefreshTargets } from './stateChanges/applySelectionRefreshTargets';

export function startAllStateChangeAppliers(): void {
  startApplyingSidebarRefreshTargets();
  startApplyingGridRefreshTargets();
  startApplyingSelectionRefreshTargets();
}

export function stopAllStateChangeAppliers(): void {
  stopApplyingSidebarRefreshTargets();
  stopApplyingGridRefreshTargets();
  stopApplyingSelectionRefreshTargets();
}
