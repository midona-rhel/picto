import * as api from '../platform/api';

export const windowController = {
  openDetailWindow(input: {
    hash: string;
    width?: number | null;
    height?: number | null;
  }): Promise<void> {
    return api.openDetailWindow(input);
  },
};
