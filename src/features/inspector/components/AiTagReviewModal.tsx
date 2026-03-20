import { useEffect, useMemo, useState } from 'react';
import { Button, Checkbox, Collapse, Group, Loader, Modal, Text, UnstyledButton } from '@mantine/core';
import { IconChevronDown, IconChevronRight } from '@tabler/icons-react';
import { api } from '#desktop/api';
import type { AiFilePrediction, AiTagPrediction } from '../../../shared/types/api';

interface AiTagReviewModalProps {
  opened: boolean;
  onClose: () => void;
  hashes: string[];
  onApply: (tags: string[]) => Promise<void>;
}

interface GroupState {
  expanded: boolean;
}

const NAMESPACE_ORDER = ['general', 'character', 'copyright', 'artist', 'species', 'rating'];
const NAMESPACE_LABELS: Record<string, string> = {
  general: 'General',
  character: 'Character',
  copyright: 'Copyright / Series',
  artist: 'Artist',
  species: 'Species',
  rating: 'Rating',
};

export function AiTagReviewModal({ opened, onClose, hashes, onApply }: AiTagReviewModalProps) {
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState(false);
  const [predictions, setPredictions] = useState<AiFilePrediction[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [groups, setGroups] = useState<Record<string, GroupState>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (opened && hashes.length > 0) {
      void runPrediction();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opened]);

  const runPrediction = async () => {
    setLoading(true);
    setError(null);
    setPredictions([]);
    setSelected(new Set());
    try {
      const result = await api.aiTagger.predict(hashes);
      setPredictions(result.predictions);

      // Select all predicted tags by default
      const allTags = new Set<string>();
      for (const pred of result.predictions) {
        for (const tag of pred.tags) {
          allTags.add(`${tag.namespace}:${tag.tag}`);
        }
      }
      setSelected(allTags);

      // Expand all groups by default
      const gs: Record<string, GroupState> = {};
      for (const ns of NAMESPACE_ORDER) {
        gs[ns] = { expanded: true };
      }
      setGroups(gs);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  // Merge tags from all predictions, deduplicating and keeping highest confidence
  const mergedTags = useMemo(() => {
    const tagMap = new Map<string, AiTagPrediction>();
    for (const pred of predictions) {
      for (const tag of pred.tags) {
        const key = `${tag.namespace}:${tag.tag}`;
        const existing = tagMap.get(key);
        if (!existing || tag.confidence > existing.confidence) {
          tagMap.set(key, tag);
        }
      }
    }
    return Array.from(tagMap.values());
  }, [predictions]);

  // Group by namespace
  const grouped = useMemo(() => {
    const map = new Map<string, AiTagPrediction[]>();
    for (const tag of mergedTags) {
      const list = map.get(tag.namespace) ?? [];
      list.push(tag);
      map.set(tag.namespace, list);
    }
    // Sort each group by confidence descending
    for (const list of map.values()) {
      list.sort((a, b) => b.confidence - a.confidence);
    }
    return map;
  }, [mergedTags]);

  const toggleTag = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const toggleGroup = (ns: string) => {
    const tags = grouped.get(ns) ?? [];
    const keys = tags.map((t) => `${t.namespace}:${t.tag}`);
    const allSelected = keys.every((k) => selected.has(k));
    setSelected((prev) => {
      const next = new Set(prev);
      for (const k of keys) {
        if (allSelected) next.delete(k);
        else next.add(k);
      }
      return next;
    });
  };

  const toggleGroupExpand = (ns: string) => {
    setGroups((prev) => ({
      ...prev,
      [ns]: { expanded: !(prev[ns]?.expanded ?? true) },
    }));
  };

  const handleApply = async () => {
    setApplying(true);
    try {
      await onApply(Array.from(selected));
      onClose();
    } catch (err) {
      console.error('Failed to apply AI tags:', err);
    } finally {
      setApplying(false);
    }
  };

  const orderedNamespaces = NAMESPACE_ORDER.filter((ns) => grouped.has(ns));

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title="AI Tag Predictions"
      size="lg"
      centered
    >
      {loading ? (
        <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
          <Loader size="sm" />
          <Text ml="sm" size="sm" c="dimmed">Analyzing {hashes.length} {hashes.length === 1 ? 'image' : 'images'}...</Text>
        </div>
      ) : error ? (
        <Text c="red" size="sm">{error}</Text>
      ) : mergedTags.length === 0 ? (
        <Text c="dimmed" size="sm" ta="center" py={40}>No tags predicted above threshold.</Text>
      ) : (
        <>
          <div style={{ maxHeight: 400, overflowY: 'auto' }}>
            {orderedNamespaces.map((ns) => {
              const tags = grouped.get(ns) ?? [];
              const keys = tags.map((t) => `${t.namespace}:${t.tag}`);
              const selectedCount = keys.filter((k) => selected.has(k)).length;
              const allSelected = selectedCount === keys.length;
              const expanded = groups[ns]?.expanded ?? true;

              return (
                <div key={ns} style={{ marginBottom: 8 }}>
                  <Group gap={4} style={{ cursor: 'pointer', userSelect: 'none' }}>
                    <UnstyledButton onClick={() => toggleGroupExpand(ns)} style={{ display: 'flex', alignItems: 'center' }}>
                      {expanded ? <IconChevronDown size={14} /> : <IconChevronRight size={14} />}
                    </UnstyledButton>
                    <Checkbox
                      checked={allSelected}
                      indeterminate={selectedCount > 0 && !allSelected}
                      onChange={() => toggleGroup(ns)}
                      size="xs"
                    />
                    <Text size="sm" fw={600} onClick={() => toggleGroupExpand(ns)} style={{ cursor: 'pointer' }}>
                      {NAMESPACE_LABELS[ns] ?? ns} ({tags.length})
                    </Text>
                  </Group>

                  <Collapse in={expanded}>
                    <div style={{ paddingLeft: 36, paddingTop: 4 }}>
                      {tags.map((tag) => {
                        const key = `${tag.namespace}:${tag.tag}`;
                        return (
                          <Group key={key} gap={8} py={2} wrap="nowrap">
                            <Checkbox
                              checked={selected.has(key)}
                              onChange={() => toggleTag(key)}
                              size="xs"
                            />
                            <Text size="xs" style={{ flex: 1 }}>{tag.tag}</Text>
                            <Text size="xs" c="dimmed" style={{ fontVariantNumeric: 'tabular-nums' }}>
                              {Math.round(tag.confidence * 100)}%
                            </Text>
                          </Group>
                        );
                      })}
                    </div>
                  </Collapse>
                </div>
              );
            })}
          </div>

          <Group justify="flex-end" mt="md" gap={8}>
            <Button variant="subtle" onClick={onClose} size="sm">Cancel</Button>
            <Button
              onClick={handleApply}
              loading={applying}
              disabled={selected.size === 0}
              size="sm"
            >
              Apply {selected.size} {selected.size === 1 ? 'tag' : 'tags'}
            </Button>
          </Group>
        </>
      )}
    </Modal>
  );
}
