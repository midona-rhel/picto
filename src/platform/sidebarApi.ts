import { invoke } from './ipc';
import type { SidebarTreeResponse } from '../shared/types/canonical';

export function getSidebarTree(): Promise<SidebarTreeResponse> {
  return invoke<SidebarTreeResponse>('get_sidebar_tree');
}

export function reorderSidebarNodes(moves: [string, number][]): Promise<void> {
  return invoke<void>('reorder_sidebar_nodes', { moves });
}

export function pinSidebarItem(nodeId: string): Promise<void> {
  return invoke<void>('pin_sidebar_item', { node_id: nodeId });
}

export function unpinSidebarItem(nodeId: string): Promise<void> {
  return invoke<void>('unpin_sidebar_item', { node_id: nodeId });
}

export function reorderPinnedItems(moves: [string, number][]): Promise<void> {
  return invoke<void>('reorder_pinned_items', { moves });
}
