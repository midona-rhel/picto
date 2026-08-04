import { useEffect } from 'react';
import { viewerController } from '../../../controllers/viewerController';

/** Persist a view when a viewer changes to a different logical media entity. */
export function useRecordMediaView(entityHash: string | null | undefined): void {
  useEffect(() => {
    if (!entityHash) return;
    void viewerController.recordMediaView(entityHash).catch(() => {});
  }, [entityHash]);
}
