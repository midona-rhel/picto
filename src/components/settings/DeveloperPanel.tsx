import { useEffect, useState } from 'react';
import { Text } from '@mantine/core';
import { PerfController } from '../../controllers/perfController';
import type { PerfSloResult } from '../../shared/types/api';
import { TextButton } from '../../shared/components/TextButton';
import { SettingsBlock } from './ui';

function formatMs(v: unknown): string {
  const n = typeof v === 'number' ? v : Number(v);
  if (!Number.isFinite(n)) return '-';
  return `${n.toFixed(1)}ms`;
}

export function DeveloperPanel() {
  const [slo, setSlo] = useState<PerfSloResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const loadSlo = async (background = false) => {
    if (!background) setLoading(true);
    setRefreshing(background);
    try {
      const result = await PerfController.checkSlo();
      setSlo(result);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load SLO check');
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  };

  useEffect(() => {
    void loadSlo();
    const interval = setInterval(() => {
      void loadSlo(true);
    }, 10_000);
    return () => clearInterval(interval);
  }, []);

  return (
    <SettingsBlock
      title="SLO Check"
      description="Global performance SLOs for interactive flows."
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
        <Text size="xs" c="dimmed">
          {refreshing ? 'Refreshing...' : 'Auto-refresh every 10s'}
        </Text>
        <TextButton compact onClick={() => { void loadSlo(); }}>
          Refresh
        </TextButton>
      </div>

      {!slo ? (
        <Text size="xs" c="dimmed">{loading ? 'Loading SLO metrics...' : 'No SLO metrics yet.'}</Text>
      ) : (
        <>
          <Text size="xs" c={slo.pass ? 'teal' : 'red'} fw={600}>
            Overall: {slo.pass ? 'PASS' : 'FAIL'}
          </Text>
          <Text size="xs" c="dimmed" mt={2}>
            Click metadata: p50={formatMs(slo.click_metadata.p50_ms)} · p95={formatMs(slo.click_metadata.p95_ms)} · p99={formatMs(slo.click_metadata.p99_ms)}
          </Text>
          <Text size="xs" c="dimmed" mt={2}>
            Grid first page: p50={formatMs(slo.grid_first_page.p50_ms)} · p95={formatMs(slo.grid_first_page.p95_ms)} · p99={formatMs(slo.grid_first_page.p99_ms)}
          </Text>
          <Text size="xs" c="dimmed" mt={2}>
            Sidebar: p50={formatMs(slo.sidebar_tree.p50_ms)} · p95={formatMs(slo.sidebar_tree.p95_ms)} · p99={formatMs(slo.sidebar_tree.p99_ms)}
          </Text>
          {slo.missing_metrics?.length > 0 && (
            <Text size="xs" c="yellow" mt={2}>Missing metrics: {slo.missing_metrics.join(', ')}</Text>
          )}
        </>
      )}

      {error && (
        <Text size="xs" c="red" mt={6}>{error}</Text>
      )}
    </SettingsBlock>
  );
}
