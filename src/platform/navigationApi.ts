import { invoke } from './ipc';
import type { NavigationSnapshot } from '../shared/types/generated/application/NavigationSnapshot';
import type { SidebarCounts } from '../shared/types/generated/application/SidebarCounts';

export function getNavigation(): Promise<NavigationSnapshot> {
  return invoke<NavigationSnapshot>('navigation.get', {});
}

export function getSidebarCounts(): Promise<SidebarCounts> {
  return invoke<SidebarCounts>('sidebar.counts', {});
}
