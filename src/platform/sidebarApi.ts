import { invoke } from './ipc';
import type { SidebarTreeResponse } from '../shared/types/canonical';

export function getSidebarTree(): Promise<SidebarTreeResponse> {
  return invoke<SidebarTreeResponse>('get_sidebar_tree');
}

export function reorderSidebarNodes(moves: [string, number][]): Promise<void> {
  return invoke<void>('reorder_sidebar_nodes', { moves });
}
