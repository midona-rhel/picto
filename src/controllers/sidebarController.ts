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
