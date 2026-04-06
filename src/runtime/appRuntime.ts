import { startInspectorSync } from '../controllers/inspectorController';
import { startGridSettle } from './gridSettle';
import { startSidebarSettle } from './sidebarSettle';

let cleanupFns: Array<() => void> = [];

export function startAppRuntime(): () => void {
  for (const cleanup of cleanupFns) cleanup();
  cleanupFns = [
    startSidebarSettle(),
    startGridSettle(),
    startInspectorSync(),
  ];
  return () => {
    for (const cleanup of cleanupFns) cleanup();
    cleanupFns = [];
  };
}
