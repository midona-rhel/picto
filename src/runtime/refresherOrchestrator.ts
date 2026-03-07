import { startSidebarRefresher, stopSidebarRefresher } from './resourceRefreshers/sidebarRefresher';
import { startGridRefresher, stopGridRefresher } from './resourceRefreshers/gridRefresher';
import { startSelectionRefresher, stopSelectionRefresher } from './resourceRefreshers/selectionRefresher';

export function startAllRefreshers(): void {
  startSidebarRefresher();
  startGridRefresher();
  startSelectionRefresher();
}

export function stopAllRefreshers(): void {
  stopSidebarRefresher();
  stopGridRefresher();
  stopSelectionRefresher();
}
