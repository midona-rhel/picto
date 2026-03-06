import { useState, useCallback } from 'react';
import type { TagWithType } from '#features/tags/components';

export function useTagEditor(initialTags: TagWithType[] = []) {
  const [editedTags, setEditedTags] = useState<TagWithType[]>(initialTags);

  const handleRemoveTag = useCallback((tagName: string) => {
    setEditedTags(prev => prev.filter(t => t.name !== tagName));
  }, []);

  const handleAddTag = useCallback((tagName: string) => {
    setEditedTags(prev => {
      if (prev.some(t => t.name === tagName)) return prev;
      return [...prev, { name: tagName, tag_type: 'general' }];
    });
  }, []);

  const handleAddTagWithType = useCallback((tagName: string, tagType: string) => {
    setEditedTags(prev => {
      const existing = prev.find(t => t.name === tagName);
      if (existing) {
        if (existing.tag_type === tagType) return prev;
        return prev.map(t => t.name === tagName ? { ...t, tag_type: tagType } : t);
      }
      return [...prev, { name: tagName, tag_type: tagType }];
    });
  }, []);

  return {
    editedTags,
    setEditedTags,
    handleRemoveTag,
    handleAddTag,
    handleAddTagWithType,
  };
}
