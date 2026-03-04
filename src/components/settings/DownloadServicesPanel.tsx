import { useState, useEffect } from 'react';
import { NumberInput, Switch, Text, Loader } from '@mantine/core';
import { api } from '#desktop/api';
import { SettingsBlock, SettingsRow } from './ui';

interface AppSettings {
  subAbortThreshold: number;
  subInboxPauseLimit: number;
  subRateLimitSecs: number;
  subBatchSize: number;
  [key: string]: unknown;
}

export function DownloadServicesPanel() {
  const [settings, setSettings] = useState<AppSettings | null>(null);

  useEffect(() => {
    void loadSettings();
  }, []);

  const loadSettings = async () => {
    try {
      const result = await api.settings.get() as AppSettings;
      setSettings(result);
    } catch (err) {
      console.error('Failed to load settings:', err);
    }
  };

  const saveSetting = async (patch: Partial<AppSettings>) => {
    if (!settings) return;
    const updated = { ...settings, ...patch };
    setSettings(updated);
    try {
      await api.settings.save(updated);
    } catch (err) {
      console.error('Failed to save settings:', err);
    }
  };

  if (!settings) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
        <Loader size="sm" />
      </div>
    );
  }

  const unlimitedBatch = settings.subBatchSize === 0;

  return (
    <>
      <SettingsBlock title="Rate Limiting" description="Controls how aggressively gallery-dl fetches from sites.">
        <SettingsRow label="Sleep between requests">
          <NumberInput
            size="xs"
            w={100}
            value={settings.subRateLimitSecs}
            onChange={(v) => saveSetting({ subRateLimitSecs: Number(v) || 1 })}
            min={0.5}
            max={30}
            step={0.5}
            decimalScale={1}
            suffix="s"
          />
        </SettingsRow>
      </SettingsBlock>

      <SettingsBlock title="Batch Limits" description="How many files to download per subscription run.">
        <SettingsRow label="Unlimited batch downloads">
          <Switch
            size="xs"
            checked={unlimitedBatch}
            onChange={(e) => saveSetting({ subBatchSize: e.currentTarget.checked ? 0 : 100 })}
          />
        </SettingsRow>
        {!unlimitedBatch && (
          <SettingsRow label="Files per batch" separator>
            <NumberInput
              size="xs"
              w={100}
              value={settings.subBatchSize}
              onChange={(v) => saveSetting({ subBatchSize: Math.max(1, Number(v) || 100) })}
              min={1}
              max={5000}
              step={10}
            />
          </SettingsRow>
        )}
        <SettingsRow label="Abort after consecutive skips" separator>
          <NumberInput
            size="xs"
            w={100}
            value={settings.subAbortThreshold}
            onChange={(v) => saveSetting({ subAbortThreshold: Number(v) || 10 })}
            min={1}
            max={500}
            step={1}
          />
        </SettingsRow>
        <Text size="xs" c="dimmed" mt={4}>
          On repeat runs, stop early after this many already-downloaded files in a row.
        </Text>
      </SettingsBlock>

      <SettingsBlock title="Inbox Cap" description="Subscription ingestion automatically pauses at 1000 inbox items.">
        <SettingsRow label="Max inbox items">
          <Text size="sm">1000</Text>
        </SettingsRow>
      </SettingsBlock>
    </>
  );
}
