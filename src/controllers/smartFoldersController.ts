/**
 * Smart folder controller — owns smart folder actions.
 * Calls API, then refreshes sidebar tree.
 */

import * as api from '../platform/api';
import { sidebarController } from './sidebarController';

export const smartFoldersController = {
  async delete(id: string) {
    await api.deleteSmartFolder(id);
    await sidebarController.fetchTree();
  },

  // TODO: create and update need modal UI — out of scope for this chunk
};
