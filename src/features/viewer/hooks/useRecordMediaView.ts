import { useEffect } from 'react';
import { viewerController } from '../../../controllers/viewerController';

/** Persist a view when a viewer changes to a different logical media entity. */
export function useRecordMediaView(itemId: number | null | undefined): void {
  useEffect(() => {
    if (!itemId) return;
    void viewerController.recordMediaView(itemId).catch(() => {});
  }, [itemId]);
}
