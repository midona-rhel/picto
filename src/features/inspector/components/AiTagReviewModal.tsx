import { useEffect, useMemo, useState } from 'react';
import { Button, Checkbox, Group, Loader, Modal, Text } from '@mantine/core';
import { api } from '#desktop/api';
import { glassModalStyles } from '../../../shared/styles/glassModal';
import { NamespaceTagChip } from '../../../shared/components/NamespaceTagChip';
import type { AiFilePrediction, AiTagPrediction } from '../../../shared/types/api';

interface AiTagReviewModalProps {
  opened: boolean;
  onClose: () => void;
  hashes: string[];
  onApply: (tags: string[]) => Promise<void>;
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

      const allTags = new Set<string>();
      for (const pred of result.predictions) {
        for (const tag of pred.tags) {
          allTags.add(`${tag.namespace}:${tag.tag}`);
        }
      }
      setSelected(allTags);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

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

  const grouped = useMemo(() => {
    const map = new Map<string, AiTagPrediction[]>();
    for (const tag of mergedTags) {
      const list = map.get(tag.namespace) ?? [];
      list.push(tag);
      map.set(tag.namespace, list);
    }
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
      styles={glassModalStyles}
    >
      {loading ? (
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 40 }}>
          <Loader size="sm" />
          <Text ml="sm" size="sm" style={{ color: 'var(--color-text-secondary)' }}>
            Analyzing {hashes.length} {hashes.length === 1 ? 'image' : 'images'}...
          </Text>
        </div>
      ) : error ? (
        <Text c="red" size="sm">{error}</Text>
      ) : mergedTags.length === 0 ? (
        <Text size="sm" ta="center" py={40} style={{ color: 'var(--color-text-tertiary)' }}>
          No tags predicted above threshold.
        </Text>
      ) : (
        <>
          <div style={{ maxHeight: 400, overflowY: 'auto' }}>
            {orderedNamespaces.map((ns, i) => {
              const tags = grouped.get(ns) ?? [];
              return (
                <div key={ns}>
                  {/* Section header */}
                  <div style={{
                    padding: '8px 0 4px',
                    marginTop: i > 0 ? 8 : 0,
                    borderBottom: '1px solid var(--color-border-primary)',
                  }}>
                    <Text size="xs" fw={600} style={{ color: 'var(--color-text-secondary)', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
                      {NAMESPACE_LABELS[ns] ?? ns}
                    </Text>
                  </div>

                  {/* Tag rows */}
                  {tags.map((tag) => {
                    const key = `${tag.namespace}:${tag.tag}`;
                    return (
                      <div
                        key={key}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          gap: 8,
                          padding: '4px 0',
                        }}
                      >
                        <NamespaceTagChip tag={tag.tag} namespace={tag.namespace} size="sm" />
                        <Text
                          size="xs"
                          style={{
                            marginLeft: 'auto',
                            fontVariantNumeric: 'tabular-nums',
                            color: 'var(--color-text-tertiary)',
                            flexShrink: 0,
                          }}
                        >
                          {Math.round(tag.confidence * 100)}%
                        </Text>
                        <Checkbox
                          checked={selected.has(key)}
                          onChange={() => toggleTag(key)}
                          size="xs"
                          style={{ flexShrink: 0 }}
                        />
                      </div>
                    );
                  })}
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
