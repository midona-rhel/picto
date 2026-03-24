/**
 * Grid screen — feature root. Reads state, delegates to CanvasGrid.
 */

import { useEffect } from 'react';
import { useAtomValue } from 'jotai';
import { IconPhotoOff } from '@tabler/icons-react';
import { activeNodeIdAtom } from '../../state/navigation';
import {
  gridItemsAtom, gridLoadingAtom, gridErrorAtom, gridCursorAtom,
  gridViewModeAtom, gridTargetSizeAtom, gridShowNameAtom, gridShowExtensionAtom,
} from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { CanvasGrid } from './canvas/CanvasGrid';
import type { BaseScope } from '../../shared/types/canonical';
import styles from './GridScreen.module.css';

const GRID_SYSTEM_SCOPES: Record<string, string> = {
  'system:active': 'all',
  'system:inbox': 'inbox',
  'system:trash': 'trash',
  'system:uncategorized': 'uncategorized',
  'system:untagged': 'untagged',
};

const NON_GRID_NODES = new Set(['system:duplicates', 'system:recent_viewed']);

function nodeIdToScope(nodeId: string): BaseScope | null {
  if (nodeId.startsWith('folder:')) {
    const id = parseInt(nodeId.slice(7), 10);
    return { kind: 'folder', id: isNaN(id) ? 0 : id };
  }
  if (nodeId.startsWith('smart:')) {
    const id = parseInt(nodeId.slice(6), 10);
    return { kind: 'smart_folder', id: isNaN(id) ? 0 : id };
  }
  if (NON_GRID_NODES.has(nodeId)) return null;
  const scopeKey = GRID_SYSTEM_SCOPES[nodeId];
  if (scopeKey) return { kind: 'system', key: scopeKey };
  return null;
}

export function GridScreen() {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const items = useAtomValue(gridItemsAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const error = useAtomValue(gridErrorAtom);
  const cursor = useAtomValue(gridCursorAtom);
  const viewMode = useAtomValue(gridViewModeAtom);
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const showName = useAtomValue(gridShowNameAtom);
  const showExtension = useAtomValue(gridShowExtensionAtom);

  const scope = nodeIdToScope(activeNodeId);
  const isGridScope = scope !== null;

  useEffect(() => {
    if (scope) {
      gridController.navigateTo(scope);
    } else {
      gridController.deactivate();
    }
  }, [activeNodeId]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!isGridScope) {
    return <div className={styles.nonGridPlaceholder}>This view is not available yet</div>;
  }

  if (error) {
    return (
      <div className={styles.error}>
        <span>{error}</span>
        <button className={styles.retryBtn} onClick={() => gridController.loadFirstPage()}>
          Retry
        </button>
      </div>
    );
  }

  const isEmpty = items.length === 0 && !loading;

  if (isEmpty) {
    return (
      <div className={styles.empty}>
        <IconPhotoOff size={32} stroke={1} className={styles.emptyIcon} />
        <span>No items</span>
      </div>
    );
  }

  return (
    <div className={styles.root}>
      <CanvasGrid
        items={items}
        viewMode={viewMode}
        targetSize={targetSize}
        showName={showName}
        showExtension={showExtension}
        onTileClick={(_index, _item) => { /* TODO: selection / viewer — PBI-593 */ }}
        onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
      />
    </div>
  );
}
