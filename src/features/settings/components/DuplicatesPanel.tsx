import { useEffect, useState } from 'react';
import { Loader, NumberInput, Switch, Text } from '@mantine/core';
import { duplicatesController } from '../../../controllers/duplicatesController';
import type { DuplicateSettings } from '../../../shared/types/api';
import { SettingsBlock, SettingsRow } from './ui';

const MIN_SIMILARITY = 95;
const MAX_SIMILARITY = 100;

export function DuplicatesPanel() {
  const [settings, setSettings] = useState<DuplicateSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    void loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      setLoading(true);
      const data = await duplicatesController.getSettings();
      setSettings(data);
    } catch (err) {
      console.error('Failed to load duplicate settings:', err);
      setSettings(null);
    } finally {
      setLoading(false);
    }
  };

  const update = async (patch: Partial<DuplicateSettings>) => {
    if (!settings) return;
    const previous = { ...settings };
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      setSaving(true);
      await duplicatesController.updateSettings(patch, previous);
    } catch (err) {
      console.error('Failed to save duplicate settings:', err);
      // Reload server truth if save failed.
      await loadSettings();
    } finally {
      setSaving(false);
    }
  };

  if (loading || !settings) {
    return (
      <SettingsBlock title="Duplicate Detection">
        <Loader size="xs" />
      </SettingsBlock>
    );
  }

  return (
    <>
      <SettingsBlock
        title="Duplicate Detection"
        description="Detection threshold controls which pairs are discovered during scans."
        dimmed={saving}
      >
        <SettingsRow label="Detect similarity">
          <NumberInput
            size="xs"
            w={90}
            min={MIN_SIMILARITY}
            max={MAX_SIMILARITY}
            step={1}
            suffix="%"
            value={settings.duplicateDetectSimilarityPct}
            onChange={(value) => {
              const n = Number(value);
              if (!Number.isFinite(n)) return;
              void update({ duplicateDetectSimilarityPct: Math.max(MIN_SIMILARITY, Math.min(MAX_SIMILARITY, Math.round(n))) });
            }}
          />
        </SettingsRow>
        <SettingsRow label="Review similarity" separator>
          <NumberInput
            size="xs"
            w={90}
            min={MIN_SIMILARITY}
            max={MAX_SIMILARITY}
            step={1}
            suffix="%"
            value={settings.duplicateReviewSimilarityPct}
            onChange={(value) => {
              const n = Number(value);
              if (!Number.isFinite(n)) return;
              void update({ duplicateReviewSimilarityPct: Math.max(MIN_SIMILARITY, Math.min(MAX_SIMILARITY, Math.round(n))) });
            }}
          />
        </SettingsRow>
      </SettingsBlock>

      <SettingsBlock
        title="Auto Merge"
        description="Auto-merge combines metadata and keeps the higher quality file when an exact perceptual-hash match is found."
        dimmed={saving}
      >
        <SettingsRow label="Enable auto-merge">
          <Switch
            size="xs"
            checked={settings.duplicateAutoMergeEnabled}
            onChange={(e) => {
              void update({ duplicateAutoMergeEnabled: e.currentTarget.checked });
            }}
          />
        </SettingsRow>

        <SettingsRow label="Similarity" separator>
          <NumberInput
            size="xs"
            w={90}
            min={MIN_SIMILARITY}
            max={MAX_SIMILARITY}
            step={1}
            suffix="%"
            value={settings.duplicateAutoMergeSimilarityPct}
            disabled={!settings.duplicateAutoMergeEnabled}
            onChange={(value) => {
              const n = Number(value);
              if (!Number.isFinite(n)) return;
              void update({ duplicateAutoMergeSimilarityPct: Math.max(MIN_SIMILARITY, Math.min(MAX_SIMILARITY, Math.round(n))) });
            }}
          />
        </SettingsRow>

        <SettingsRow label="Require matching dimensions" separator>
          <Switch
            size="xs"
            checked={settings.duplicateAutoMergeRequireMatchingDimensions}
            disabled={!settings.duplicateAutoMergeEnabled}
            onChange={(e) => {
              void update({ duplicateAutoMergeRequireMatchingDimensions: e.currentTarget.checked });
            }}
          />
        </SettingsRow>

        <SettingsRow label="Subscriptions only" separator>
          <Switch
            size="xs"
            checked={settings.duplicateAutoMergeSubscriptionsOnly}
            disabled={!settings.duplicateAutoMergeEnabled}
            onChange={(e) => {
              void update({ duplicateAutoMergeSubscriptionsOnly: e.currentTarget.checked });
            }}
          />
        </SettingsRow>

        {!settings.duplicateAutoMergeSubscriptionsOnly && (
          <Text size="xs" c="dimmed" mt={8}>
            Manual imports will also run duplicate auto-merge.
          </Text>
        )}
        <Text size="xs" c="dimmed" mt={8}>
          Auto-merge already requires an exact pHash match. Enable matching dimensions for a stricter safety check.
        </Text>
      </SettingsBlock>
    </>
  );
}
