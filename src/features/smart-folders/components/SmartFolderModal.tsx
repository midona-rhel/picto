import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { Modal, Stack, Group, TextInput, Text, Loader, ActionIcon } from '@mantine/core';
import { glassModalStyles } from '../../../shared/styles/glassModal';
import { smartFoldersController } from '../../../controllers/smartFoldersController';
import { TextButton } from '../../../shared/components/TextButton';
import { RuleGroupEditor } from './RuleGroupEditor';
import type { SmartFolderPredicate } from './types';
import type { SmartFolder } from './types';
import { createDefaultGroup, predicateToRust, folderToRust } from './types';
import { IconPicker } from './IconPicker';
import { FolderColorPicker } from './FolderColorPicker';
import { DynamicIcon, DEFAULT_FOLDER_ICON } from './iconRegistry';
import { useDomainStore } from '../../../state-legacy/domainStore';

interface SmartFolderModalProps {
  opened: boolean;
  onClose: () => void;
  folder?: SmartFolder | null;
  initialParentId?: number | null;
  onSaved: () => void | Promise<void>;
}

function hasRules(predicate: SmartFolderPredicate): boolean {
  return predicate.groups.some((group) => group.rules.length > 0);
}

function combinePredicates(predicates: SmartFolderPredicate[]): SmartFolderPredicate {
  return {
    groups: predicates.flatMap((predicate) => predicate.groups.filter((group) => group.rules.length > 0)),
  };
}

export function SmartFolderModal({ opened, onClose, folder, initialParentId = null, onSaved }: SmartFolderModalProps) {
  const smartFolders = useDomainStore((state) => state.smartFolders);
  const [name, setName] = useState('');
  const [icon, setIcon] = useState<string | null>(null);
  const [color, setColor] = useState<string | null>(null);
  const [predicate, setPredicate] = useState<SmartFolderPredicate>({ groups: [createDefaultGroup()] });
  const [liveCount, setLiveCount] = useState<number | null>(null);
  const [counting, setCounting] = useState(false);
  const [saving, setSaving] = useState(false);
  const countTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (opened) {
      if (folder) {
        setName(folder.name);
        setIcon(folder.icon ?? null);
        setColor(folder.color ?? null);
        setPredicate(folder.predicate);
      } else {
        setName('');
        setIcon(null);
        setColor(null);
        setPredicate({ groups: [createDefaultGroup()] });
      }
      setLiveCount(null);
    }
  }, [opened, folder, initialParentId]);

  const nodeMap = useMemo(() => {
    const map = new Map<string, typeof smartFolders[number]>();
    for (const smartFolder of smartFolders) map.set(smartFolder.id, smartFolder);
    return map;
  }, [smartFolders]);

  const resolvedParentId = folder?.parent_id != null
    ? String(folder.parent_id)
    : initialParentId != null
      ? String(initialParentId)
      : null;

  const inheritedChain = useMemo(() => {
    if (!resolvedParentId) return [];
    const chain: typeof smartFolders = [];
    let currentId: string | null = resolvedParentId;
    const visited = new Set<string>();
    while (currentId && !visited.has(currentId)) {
      visited.add(currentId);
      const current = nodeMap.get(currentId);
      if (!current) break;
      chain.unshift(current);
      currentId = current.parent_id;
    }
    return chain;
  }, [nodeMap, resolvedParentId, smartFolders]);

  const effectivePredicate = useMemo(() => combinePredicates([
    ...inheritedChain.map((item) => item.localPredicate ?? item.predicate ?? { groups: [] }),
    predicate,
  ]), [inheritedChain, predicate]);

  const updateCount = useCallback((pred: SmartFolderPredicate) => {
    if (countTimer.current) clearTimeout(countTimer.current);
    countTimer.current = setTimeout(async () => {
      if (!hasRules(pred)) {
        setLiveCount(null);
        return;
      }
      setCounting(true);
      try {
        const count = await smartFoldersController.count(predicateToRust(pred));
        setLiveCount(count);
      } catch (e) {
        console.error('Count failed:', e);
        setLiveCount(null);
      } finally {
        setCounting(false);
      }
    }, 500);
  }, []);

  useEffect(() => {
    if (opened) updateCount(effectivePredicate);
  }, [effectivePredicate, opened, updateCount]);

  const handleGroupChange = (index: number, group: SmartFolderPredicate['groups'][0]) => {
    const groups = [...predicate.groups];
    groups[index] = group;
    setPredicate({ groups });
  };

  const handleGroupRemove = (index: number) => {
    setPredicate({ groups: predicate.groups.filter((_, i) => i !== index) });
  };

  const handleAddGroup = () => {
    setPredicate({ groups: [...predicate.groups, createDefaultGroup()] });
  };

  const handleSave = async () => {
    if (!name.trim()) return;
    setSaving(true);
    try {
      const folderData = folderToRust({
        name: name.trim(),
        parent_id: resolvedParentId ? parseInt(resolvedParentId, 10) : null,
        icon,
        color,
        predicate,
      });

      if (folder?.id) {
        const beforeData = folderToRust({
          name: folder.name,
          parent_id: folder.parent_id ?? null,
          icon: folder.icon ?? null,
          color: folder.color ?? null,
          predicate: folder.predicate,
          sort_field: folder.sort_field,
          sort_order: folder.sort_order,
        });
        await smartFoldersController.update(folder.id!, folderData, beforeData);
      } else {
        await smartFoldersController.create(folderData);
      }

      await onSaved();
      onClose();
    } catch (e) {
      console.error('Save failed:', e);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title={folder ? 'Edit Smart Folder' : 'New Smart Folder'}
      size="lg"
      centered
      styles={{
        ...glassModalStyles,
        title: { fontWeight: 600, fontSize: 'var(--mantine-font-size-lg)' },
        body: { padding: 'var(--mantine-spacing-lg)' },
      }}
    >
      <Stack gap="md">
        {/* Name section */}
        <div>
          <Text size="sm" fw={500} mb={6}>Name</Text>
          <TextInput
            placeholder="Smart folder name..."
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            size="sm"
          />
        </div>

        {inheritedChain.length > 0 && (
          <div>
            <Text size="sm" fw={500} mb={6}>Parent</Text>
            <Text size="sm" c="dimmed">
              {inheritedChain.map((item) => item.name).join(' / ')}
            </Text>
          </div>
        )}

        {/* Icon & Color */}
        <Group gap="xl">
          <div>
            <Text size="sm" fw={500} mb={6}>Icon</Text>
            <IconPicker value={icon} onChange={setIcon}>
              <ActionIcon variant="light" color="gray" size="lg">
                <DynamicIcon name={icon ?? DEFAULT_FOLDER_ICON} size={18} color={color ?? undefined} />
              </ActionIcon>
            </IconPicker>
          </div>
          <div>
            <Text size="sm" fw={500} mb={6}>Color</Text>
            <FolderColorPicker value={color} onChange={setColor} />
          </div>
        </Group>

        {inheritedChain.length > 0 && (
          <div>
            <Text size="sm" fw={500} mb={6}>Inherited Rules</Text>
            <Text size="sm" c="dimmed">
              From {inheritedChain.map((item) => item.name).join(' / ')}
            </Text>
            <Text size="xs" c="dimmed" mt={4}>
              {combinePredicates(inheritedChain.map((item) => item.localPredicate ?? item.predicate ?? { groups: [] })).groups.length} inherited rule group(s)
            </Text>
          </div>
        )}

        {!hasRules(predicate) && (
          <Text size="sm" c="dimmed">
            This smart folder has no local rules and will behave as an organizer unless it inherits rules from a parent.
          </Text>
        )}

        {/* Rule groups */}
        {predicate.groups.map((group, i) => (
          <div key={i}>
            {i > 0 && (
              <Text size="xs" c="dimmed" mb={6}>and</Text>
            )}
            <RuleGroupEditor
              group={group}
              onChange={(g) => handleGroupChange(i, g)}
              onRemove={() => handleGroupRemove(i)}
              onAddGroup={handleAddGroup}
              canRemove={predicate.groups.length > 1}
            />
          </div>
        ))}

        {/* Footer: count + buttons */}
        <Group justify="space-between" mt="xs">
          <div>
            {counting ? (
              <Group gap={4}>
                <Loader size={12} />
                <Text size="sm" c="dimmed">Counting...</Text>
              </Group>
            ) : liveCount != null ? (
              <Text size="sm" c="dimmed">
                <Text span fw={600}>{liveCount.toLocaleString()}</Text> {liveCount === 1 ? 'item' : 'items'} found
              </Text>
            ) : (
              <Text size="sm" c="dimmed">Organizer only</Text>
            )}
          </div>

          <div style={{ display: 'flex', gap: 6 }}>
            <TextButton onClick={handleSave} disabled={!name.trim() || saving}>
              {folder ? 'Update' : 'Create'}
            </TextButton>
            <TextButton onClick={onClose}>
              Cancel
            </TextButton>
          </div>
        </Group>
      </Stack>
    </Modal>
  );
}
