import { startAppSettingsSettle } from './appSettingsSettle';
import { startGridSettle } from './gridSettle';
import { startInspectorSettle } from './inspectorSettle';
import { startSidebarSettle } from './sidebarSettle';

let cleanupFns: Array<() => void> = [];

export function startAppRuntime(): () => void {
  for (const cleanup of cleanupFns) cleanup();
  cleanupFns = [
    startAppSettingsSettle(),
    startSidebarSettle(),
    startGridSettle(),
    startInspectorSettle(),
  ];
  return () => {
    for (const cleanup of cleanupFns) cleanup();
    cleanupFns = [];
  };
}
