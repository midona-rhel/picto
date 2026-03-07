import { useState } from 'react';
import {
  Group,
  ActionIcon,
  TextInput,
  Popover,
  Text,
} from '@mantine/core';
import { IconPlus } from '@tabler/icons-react';
import { NamespaceTagChip } from '../shared/components/NamespaceTagChip';

export interface TagWithType {
  name: string;
  tag_type: string;
  read_only?: boolean;
}

/** Map booru-style tag types to Hydrus-style namespaces for consistent coloring */
const TAG_TYPE_TO_NAMESPACE: Record<string, string> = {
  artist: 'creator',
  character: 'character',
  copyright: 'series',
  metadata: 'meta',
  general: '',
};

const TAG_TYPE_ORDER: Record<string, number> = {
  artist: 0,
  copyright: 1,
  character: 2,
  metadata: 3,
  general: 4,
};

function sortTags(tags: TagWithType[]): TagWithType[] {
  return [...tags].sort((a, b) => {
    const orderA = TAG_TYPE_ORDER[a.tag_type] ?? 5;
    const orderB = TAG_TYPE_ORDER[b.tag_type] ?? 5;
    if (orderA !== orderB) return orderA - orderB;
    return a.name.localeCompare(b.name);
  });
}

interface TagChipsProps {
  tags: TagWithType[];
  onRemove?: (tagName: string) => void;
  onAdd?: (tagName: string) => void;
  editable?: boolean;
}

export function TagChips({ tags, onRemove, onAdd, editable = false }: TagChipsProps) {
  const [addOpen, setAddOpen] = useState(false);
  const [newTag, setNewTag] = useState('');

  const sorted = sortTags(tags);

  const handleAdd = () => {
    const trimmed = newTag.trim();
    if (trimmed && onAdd) {
      onAdd(trimmed);
      setNewTag('');
      setAddOpen(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleAdd();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setAddOpen(false);
      setNewTag('');
    }
  };

  // Group tags by type for display
  const grouped = new Map<string, TagWithType[]>();
  for (const tag of sorted) {
    const existing = grouped.get(tag.tag_type) || [];
    existing.push(tag);
    grouped.set(tag.tag_type, existing);
  }

  return (
    <div>
      {Array.from(grouped.entries()).map(([tagType, typeTags]) => (
        <div key={tagType} style={{ marginBottom: 6 }}>
          <Text size="xs" c="dimmed" tt="capitalize" mb={4}>
            {tagType}
          </Text>
          <Group gap={4} wrap="wrap">
            {typeTags.map((tag) => (
              <NamespaceTagChip
                key={tag.name}
                tag={tag.name.replace(/_/g, ' ')}
                namespace={TAG_TYPE_TO_NAMESPACE[tag.tag_type] ?? ''}
                onRemove={editable && onRemove && !tag.read_only ? () => onRemove(tag.name) : undefined}
              />
            ))}
          </Group>
        </div>
      ))}

      {editable && onAdd && (
        <Popover opened={addOpen} onChange={setAddOpen} position="bottom-start">
          <Popover.Target>
            <ActionIcon
              variant="light"
              mt={4}
              onClick={() => setAddOpen(true)}
            >
              <IconPlus size={14} />
            </ActionIcon>
          </Popover.Target>
          <Popover.Dropdown>
            <TextInput
              placeholder="Tag name..."
              size="xs"
              value={newTag}
              onChange={(e) => setNewTag(e.currentTarget.value)}
              onKeyDown={handleKeyDown}
              autoFocus
              rightSection={
                <ActionIcon
                  size="xs"
                  variant="filled"
                  onClick={handleAdd}
                  disabled={!newTag.trim()}
                >
                  <IconPlus size={14} />
                </ActionIcon>
              }
            />
          </Popover.Dropdown>
        </Popover>
      )}
    </div>
  );
}
