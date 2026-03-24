/**
 * Inline rename hook — manages rename state for sidebar items.
 * Enter commits, Escape cancels, blur commits.
 */

import { useState, useRef, useCallback, useEffect } from 'react';

interface UseInlineRenameOptions {
  onCommit: (id: string, newName: string) => void;
}

export function useInlineRename({ onCommit }: UseInlineRenameOptions) {
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  const startRename = useCallback((id: string, currentName: string) => {
    setRenamingId(id);
    setRenameValue(currentName);
  }, []);

  const commitRename = useCallback(() => {
    if (renamingId && renameValue.trim()) {
      onCommit(renamingId, renameValue.trim());
    }
    setRenamingId(null);
  }, [renamingId, renameValue, onCommit]);

  const cancelRename = useCallback(() => {
    setRenamingId(null);
  }, []);

  // Focus and select input when rename starts
  useEffect(() => {
    if (renamingId && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [renamingId]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') { e.preventDefault(); commitRename(); }
    else if (e.key === 'Escape') { e.preventDefault(); cancelRename(); }
  }, [commitRename, cancelRename]);

  return {
    renamingId,
    renameValue,
    setRenameValue,
    inputRef,
    startRename,
    commitRename,
    cancelRename,
    handleKeyDown,
  };
}
