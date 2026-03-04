import { useState, useCallback, useRef, useEffect } from 'react';

/**
 * Extracts the inline rename pattern shared across FolderTree,
 * SmartFolderList, and FlowsWorking.
 */
export function useInlineRename(
  onCommit: (id: string, newName: string) => Promise<void> | void,
): {
  renamingId: string | null;
  renameValue: string;
  startRename: (id: string, currentName: string) => void;
  setRenameValue: (v: string) => void;
  commitRename: () => void;
  cancelRename: () => void;
  renameInputRef: React.RefObject<HTMLInputElement>;
  renameKeyHandler: (e: React.KeyboardEvent) => void;
} {
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [renameSeq, setRenameSeq] = useState(0);
  const renameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (renamingId) {
      // Retry focus until the input is mounted (sidebar may still be refreshing)
      let attempts = 0;
      const tryFocus = () => {
        if (renameInputRef.current) {
          renameInputRef.current.focus();
          renameInputRef.current.select();
        } else if (attempts < 10) {
          attempts++;
          setTimeout(tryFocus, 30);
        }
      };
      setTimeout(tryFocus, 0);
    }
  }, [renamingId, renameSeq]);

  const startRename = useCallback((id: string, currentName: string) => {
    setRenamingId(id);
    setRenameValue(currentName);
    setRenameSeq((s) => s + 1);
  }, []);

  const commitRename = useCallback(async () => {
    if (!renamingId || !renameValue.trim()) {
      setRenamingId(null);
      return;
    }
    await onCommit(renamingId, renameValue.trim());
    setRenamingId(null);
  }, [renamingId, renameValue, onCommit]);

  const cancelRename = useCallback(() => {
    setRenamingId(null);
  }, []);

  const renameKeyHandler = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') commitRename();
    if (e.key === 'Escape') cancelRename();
  }, [commitRename, cancelRename]);

  return {
    renamingId,
    renameValue,
    startRename,
    setRenameValue,
    commitRename,
    cancelRename,
    renameInputRef,
    renameKeyHandler,
  };
}
