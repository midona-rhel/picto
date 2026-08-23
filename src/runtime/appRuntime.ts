import { startAppSettingsSettle } from './appSettingsSettle';
import { startGridSettle } from './gridSettle';
import { startInspectorSettle } from './inspectorSettle';
import { startSidebarSettle } from './sidebarSettle';
import { libraryInvalidation } from './libraryInvalidation';

let cleanupFns: Array<() => void> = [];

export function startAppRuntime(): () => void {
  for (const cleanup of cleanupFns) cleanup();
  libraryInvalidation.start();
  cleanupFns = [
    startAppSettingsSettle(),
    startSidebarSettle(),
    startGridSettle(),
    startInspectorSettle(),
  ];
  return () => {
    for (const cleanup of cleanupFns) cleanup();
    cleanupFns = [];
    libraryInvalidation.stop();
  };
}
