import { useCallback, useEffect, useRef, useState } from 'react';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { notifyError } from '../../../shared/lib/notify';
import { api } from '#desktop/api';
import type { MediaItem } from '../shared';

export function useGridInlineRename(args: {
  singleSelectedHash: string | null;
  stateRef: React.MutableRefObject<{
    images: MediaItem[];
  }>;
}) {
  const { singleSelectedHash, stateRef } = args;
  const [renamingHash, setRenamingHash] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const renameInputRef = useRef<HTMLInputElement>(null);
  const renameCancelledRef = useRef(false);
  const renamingHashRef = useRef<string | null>(null);
  renamingHashRef.current = renamingHash;

  useEffect(() => {
    if (!renamingHash) return;
    let attempts = 0;
    const tryFocus = () => {
      if (renameInputRef.current) {
        renameInputRef.current.focus();
        renameInputRef.current.select();
      } else if (attempts < 10) {
        attempts += 1;
        setTimeout(tryFocus, 30);
      }
    };
    setTimeout(tryFocus, 0);
  }, [renamingHash]);

  const startInlineRename = useCallback(() => {
    if (!singleSelectedHash) return;
    const image = stateRef.current.images.find((item) => item.hash === singleSelectedHash);
    renameCancelledRef.current = false;
    setRenameValue(image?.name ?? '');
    setRenamingHash(singleSelectedHash);
  }, [singleSelectedHash, stateRef]);

  const cancelRename = useCallback(() => {
    renameCancelledRef.current = true;
    setRenamingHash(null);
  }, []);

  const commitRename = useCallback(() => {
    if (renameCancelledRef.current) return;
    const hash = renamingHashRef.current;
    if (!hash) return;
    const image = stateRef.current.images.find((item) => item.hash === hash);
    const before = image?.name || null;
    const after = renameValue.trim() || null;
    setRenamingHash(null);
    if (after === before) return;
    api.files
      .setName(hash, after)
      .then(() => {
        registerUndoAction({
          label: 'Rename file',
          undo: () => api.files.setName(hash, before),
          redo: () => api.files.setName(hash, after),
        });
      })
      .catch((err) => notifyError(err, 'Rename Failed'));
  }, [renameValue, stateRef]);

  useEffect(() => {
    if (renamingHash && singleSelectedHash !== renamingHash) {
      setRenamingHash(null);
    }
  }, [renamingHash, singleSelectedHash]);

  return {
    renamingHash,
    renamingHashRef,
    renameValue,
    renameInputRef,
    renameCancelledRef,
    setRenameValue,
    setRenamingHash,
    startInlineRename,
    commitRename,
    cancelRename,
  };
}
