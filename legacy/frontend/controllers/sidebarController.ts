/** Sidebar tree structure reads and reordering.
 *  Kept separate: layout-level concern shared by folders + smart folders,
 *  doesn't belong in either domain controller. */
import { api } from '#desktop/api';
import type { SidebarTreeResponse } from '../shared/types/api';

export const sidebarController = {
  getTree(): Promise<SidebarTreeResponse> {
    return api.sidebar.getTree();
  },

  reorderNodes(moves: [string, number][]) {
    return api.sidebar.reorderNodes(moves);
  },
};
