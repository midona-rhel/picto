import { api } from '#desktop/api';
import type { SelectionQuerySpec } from '../../types/api';
import { applyLifecycleMutationEffects } from './mutationEffects';

type LifecycleOptions = {
  gridReload?: (() => void) | null;
};

export async function setFileStatusWithLifecycleEffects(
  hash: string,
  status: 'active' | 'inbox' | 'trash',
  options: LifecycleOptions = {},
): Promise<void> {
  await api.file.setStatus(hash, status);
  applyLifecycleMutationEffects(options.gridReload ?? undefined);
}

export async function setStatusSelectionWithLifecycleEffects(
  selection: SelectionQuerySpec,
  status: 'active' | 'inbox' | 'trash',
  options: LifecycleOptions = {},
): Promise<number> {
  const count = await api.file.setStatusSelection(selection, status);
  applyLifecycleMutationEffects(options.gridReload ?? undefined);
  return count;
}

export async function deleteSelectionWithLifecycleEffects(
  selection: SelectionQuerySpec,
  options: LifecycleOptions = {},
): Promise<number> {
  const count = await api.file.deleteSelection(selection);
  applyLifecycleMutationEffects(options.gridReload ?? undefined);
  return count;
}

export async function deleteHashesWithLifecycleEffects(
  hashes: string[],
  options: LifecycleOptions = {},
): Promise<number> {
  const count = await api.file.deleteMany(hashes);
  applyLifecycleMutationEffects(options.gridReload ?? undefined);
  return count;
}
