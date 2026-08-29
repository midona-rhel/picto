import { invoke } from './ipc';
import type {
  CanonicalNavigationSnapshot,
  CanonicalSidebarCounts,
} from '../shared/types/canonical';

export function getNavigation(): Promise<CanonicalNavigationSnapshot> {
  return invoke<CanonicalNavigationSnapshot>('navigation.get', {});
}

export function getSidebarCounts(): Promise<CanonicalSidebarCounts> {
  return invoke<CanonicalSidebarCounts>('sidebar.counts', {});
}
