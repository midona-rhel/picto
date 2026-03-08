import { api } from '#desktop/api';
import type { SelectionQuerySpec } from '../types/api';

export async function setFileStatusWithLifecycleEffects(
  hash: string,
  status: 'active' | 'inbox' | 'trash',
): Promise<void> {
  await api.file.setStatus(hash, status);
}

export async function setStatusSelectionWithLifecycleEffects(
  selection: SelectionQuerySpec,
  status: 'active' | 'inbox' | 'trash',
): Promise<number> {
  return api.file.setStatusSelection(selection, status);
}

export async function deleteSelectionWithLifecycleEffects(
  selection: SelectionQuerySpec,
): Promise<number> {
  return api.file.deleteSelection(selection);
}

export async function deleteHashesWithLifecycleEffects(
  hashes: string[],
): Promise<number> {
  return api.file.deleteMany(hashes);
}
