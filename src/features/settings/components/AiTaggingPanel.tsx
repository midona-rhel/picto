import { useEffect, useState } from 'react';
import { Badge, Button, Loader, Slider, Switch, Text } from '@mantine/core';
import { api } from '#desktop/api';
import type { AppSettings, AiTaggerStatus, AiTaggerModelStatus } from '../../../shared/types/api';
import { SettingsBlock, SettingsRow } from './ui';

export function AiTaggingPanel() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [status, setStatus] = useState<AiTaggerStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [downloading, setDownloading] = useState<Set<string>>(new Set());

  useEffect(() => {
    void loadAll();
  }, []);

  const loadAll = async () => {
    try {
      setLoading(true);
      const [s, st] = await Promise.all([
        api.settings.get(),
        api.aiTagger.status(),
      ]);
      setSettings(s);
      setStatus(st);
    } catch (err) {
      console.error('Failed to load AI tagging settings:', err);
    } finally {
      setLoading(false);
    }
  };

  const update = async (patch: Partial<AppSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      await api.settings.save(patch);
      // Refresh status when toggles change
      if ('aiTaggerWd14Enabled' in patch || 'aiTaggerE621Enabled' in patch) {
        const st = await api.aiTagger.status();
        setStatus(st);
      }
    } catch (err) {
      console.error('Failed to save AI tagging settings:', err);
      await loadAll();
    }
  };

  const handleDownload = async (slug: string) => {
    setDownloading((prev) => new Set(prev).add(slug));
    try {
      await api.aiTagger.downloadModel(slug);
      const poll = setInterval(async () => {
        const st = await api.aiTagger.status();
        setStatus(st);
        const model = st.models.find((m) => m.slug === slug);
        if (model?.downloaded) {
          clearInterval(poll);
          setDownloading((prev) => {
            const next = new Set(prev);
            next.delete(slug);
            return next;
          });
        }
      }, 2000);
    } catch (err) {
      console.error('Model download failed:', err);
      setDownloading((prev) => {
        const next = new Set(prev);
        next.delete(slug);
        return next;
      });
    }
  };

  if (loading || !settings) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
        <Loader size="sm" />
      </div>
    );
  }

  const wd14 = status?.models.find((m) => m.slug.startsWith('wd14'));
  const e621 = status?.models.find((m) => m.slug.startsWith('z3d-e621'));
  const anyEnabled = settings.aiTaggerWd14Enabled || settings.aiTaggerE621Enabled;

  return (
    <>
      <SettingsBlock title="Models" description="Enable one or both taggers. WD14 covers general anime/illustration tags. E621 covers species and furry-specific tags.">
        {wd14 && (
          <ModelRow
            model={wd14}
            enabled={settings.aiTaggerWd14Enabled}
            onToggle={(v) => void update({ aiTaggerWd14Enabled: v })}
            downloading={downloading.has(wd14.slug)}
            onDownload={() => handleDownload(wd14.slug)}
          />
        )}
        {e621 && (
          <ModelRow
            model={e621}
            enabled={settings.aiTaggerE621Enabled}
            onToggle={(v) => void update({ aiTaggerE621Enabled: v })}
            downloading={downloading.has(e621.slug)}
            onDownload={() => handleDownload(e621.slug)}
            separator
          />
        )}
        {status?.gpuBackend && (
          <SettingsRow label="GPU backend" separator>
            <Badge variant="light" size="sm">{status.gpuBackend}</Badge>
          </SettingsRow>
        )}
      </SettingsBlock>

      <SettingsBlock title="Thresholds" description="Minimum confidence required to suggest a tag. Higher values reduce false positives.">
        <ThresholdRow
          label="General tags"
          value={settings.aiThresholdGeneral}
          onChange={(v) => void update({ aiThresholdGeneral: v })}
        />
        <ThresholdRow
          label="Character"
          value={settings.aiThresholdCharacter}
          onChange={(v) => void update({ aiThresholdCharacter: v })}
          separator
        />
        <ThresholdRow
          label="Copyright / Series"
          value={settings.aiThresholdCopyright}
          onChange={(v) => void update({ aiThresholdCopyright: v })}
          separator
        />
        <ThresholdRow
          label="Artist"
          value={settings.aiThresholdArtist}
          onChange={(v) => void update({ aiThresholdArtist: v })}
          separator
        />
        <ThresholdRow
          label="Species"
          value={settings.aiThresholdSpecies}
          onChange={(v) => void update({ aiThresholdSpecies: v })}
          separator
        />
        <ThresholdRow
          label="Rating"
          value={settings.aiThresholdRating}
          onChange={(v) => void update({ aiThresholdRating: v })}
          separator
        />
      </SettingsBlock>

      <SettingsBlock title="Automation">
        <SettingsRow label="Auto-tag on import">
          <Switch
            checked={settings.aiTaggerAutoOnImport}
            onChange={(e) => void update({ aiTaggerAutoOnImport: e.currentTarget.checked })}
            disabled={!anyEnabled}
          />
        </SettingsRow>
        <Text size="xs" c="dimmed" mt={4}>
          Automatically predict and apply tags when importing files. Requires at least one model enabled.
        </Text>
      </SettingsBlock>
    </>
  );
}

function ModelRow({
  model,
  enabled,
  onToggle,
  downloading,
  onDownload,
  separator,
}: {
  model: AiTaggerModelStatus;
  enabled: boolean;
  onToggle: (v: boolean) => void;
  downloading: boolean;
  onDownload: () => void;
  separator?: boolean;
}) {
  return (
    <>
      <SettingsRow label={model.label} separator={separator}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {model.downloaded ? (
            <Badge color="green" variant="light" size="xs">Ready</Badge>
          ) : downloading ? (
            <Loader size="xs" />
          ) : (
            <Button size="compact-xs" variant="light" onClick={onDownload}>
              Download
            </Button>
          )}
          <Switch
            checked={enabled}
            onChange={(e) => onToggle(e.currentTarget.checked)}
            disabled={!model.downloaded && !downloading}
          />
        </div>
      </SettingsRow>
    </>
  );
}

function ThresholdRow({
  label,
  value,
  onChange,
  separator,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  separator?: boolean;
}) {
  return (
    <SettingsRow label={label} separator={separator}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, width: 180 }}>
        <Slider
          min={0}
          max={1}
          step={0.01}
          value={value}
          onChange={onChange}
          style={{ flex: 1 }}
          size="xs"
          label={(v) => `${Math.round(v * 100)}%`}
        />
        <Text size="xs" w={36} ta="right" style={{ fontVariantNumeric: 'tabular-nums' }}>
          {Math.round(value * 100)}%
        </Text>
      </div>
    </SettingsRow>
  );
}
