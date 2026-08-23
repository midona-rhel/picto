import { libraryInvalidation } from './libraryInvalidation';
import { sidebarController } from '../controllers/sidebarController';

/** Re-read canonical navigation/count data after sidebar-owned mutations. */
export function startSidebarSettle(): () => void {
  const refresh = () => {
    void sidebarController.fetchTree().catch((error) => {
      console.error('sidebar refresh failed', error);
    });
  };
  const unregister = [
    libraryInvalidation.register('sidebar', refresh),
    libraryInvalidation.register('folders', refresh),
    libraryInvalidation.register('smart_folders', refresh),
  ];

  return () => {
    unregister.forEach((remove) => remove());
  };
}
