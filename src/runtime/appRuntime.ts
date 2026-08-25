import { startAppSettingsSettle } from './appSettingsSettle';
import { startApplicationMenuRuntime } from './applicationMenuRuntime';
import { startGridSettle } from './gridSettle';
import { startHistoryRuntime } from './historyRuntime';
import { startInspectorSettle } from './inspectorSettle';
import { startSidebarSettle } from './sidebarSettle';
import {
  refreshSubscriptionsWorkspace,
  startSubscriptionsSettle,
} from './subscriptionsSettle';
import { libraryInvalidation } from './libraryInvalidation';
import { startDiagnosticsRuntime } from './diagnosticsRuntime';

let cleanupFns: Array<() => void> = [];

export function startAppRuntime(): () => void {
  for (const cleanup of cleanupFns) cleanup();
  libraryInvalidation.start();
  void refreshSubscriptionsWorkspace();
  cleanupFns = [
    startAppSettingsSettle(),
    startApplicationMenuRuntime(),
    startDiagnosticsRuntime(),
    startSidebarSettle(),
    startGridSettle(),
    startHistoryRuntime(),
    startInspectorSettle(),
    startSubscriptionsSettle(),
  ];
  return () => {
    for (const cleanup of cleanupFns) cleanup();
    cleanupFns = [];
    libraryInvalidation.stop();
  };
}
