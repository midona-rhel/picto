import { useState, useEffect } from 'react';
import { Text, Loader, Select, NumberInput, Switch } from '@mantine/core';
import { api } from '#desktop/api';
import { useSettingsStore, themeToColorScheme, type ReverseSearchEngine } from '../../../state/settingsStore';
import { formatFileSize } from '../../../shared/lib/formatters';
import { runCriticalAction } from '../../../shared/lib/asyncOps';
import { TextButton } from '../../../shared/components/TextButton';
import { SettingsBlock, SettingsRow, SettingsButtonRow } from './ui';
import type { FileStats } from '../../../shared/types/api';
import styles from './Settings.module.css';

const THEMES = [ // pbi-052-suppress
  { name: 'Auto', css: 'auto', color: undefined }, // pbi-052-suppress
  { name: 'Light', css: 'light', color: '#ffffff' }, // pbi-052-suppress
  { name: 'Light Gray', css: 'lightgray', color: '#808080' }, // pbi-052-suppress
  { name: 'Gray', css: 'gray', color: '#444444' }, // pbi-052-suppress
  { name: 'Dark', css: 'dark', color: '#010101' }, // pbi-052-suppress
  { name: 'Blue', css: 'blue', color: '#28356e' }, // pbi-052-suppress
  { name: 'Purple', css: 'purple', color: '#463275' }, // pbi-052-suppress
] as const;

const ZOOM_OPTIONS = [
  { value: '75', label: '75%' },
  { value: '80', label: '80%' },
  { value: '90', label: '90%' },
  { value: '100', label: '100%' },
  { value: '110', label: '110%' },
  { value: '125', label: '125%' },
  { value: '150', label: '150%' },
];

export function GeneralPanel() {
  const { settings, updateSetting, updateSettings } = useSettingsStore();

  const activeTheme = settings.theme ?? 'dark';
  const [zoom, setZoom] = useState('100');

  const handleThemeChange = (css: string) => {
    const newTheme = css as typeof settings.theme;
    const scheme = themeToColorScheme(newTheme);
    // Set theme attributes synchronously so CSS vars apply before next paint
    const resolved = newTheme === 'auto'
      ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
      : newTheme;
    const lightThemes = new Set(['light', 'lightgray']);
    document.documentElement.dataset.theme = newTheme === 'auto' ? '' : newTheme;
    document.documentElement.dataset.mantineColorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
    document.documentElement.style.colorScheme = lightThemes.has(resolved) ? 'light' : 'dark';
    // Persist to localStorage for instant theme on reload
    localStorage.setItem('picto-theme', newTheme);
    // Atomic store update — Mantine picks up the new scheme via forceColorScheme
    updateSettings({
      theme: newTheme,
      colorScheme: scheme === 'auto' ? 'dark' : scheme,
    });
  };

  const handleZoomChange = (value: string | null) => {
    if (!value) return;
    setZoom(value);
    const factor = Number(value) / 100;
    runCriticalAction('Zoom Failed', 'settings.setZoomFactor', api.settings.setZoomFactor(factor));
  };

  const [stats, setStats] = useState<FileStats | null>(null);
  const [statsLoading, setStatsLoading] = useState(true);
  const [statsError, setStatsError] = useState<string | null>(null);

  useEffect(() => {
    void loadStorageStats();
  }, []);

  const loadStorageStats = async () => {
    try {
      setStatsLoading(true);
      setStatsError(null);
      const result = await api.stats.getImageStorageStats();
      setStats(result);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setStatsError(msg);
    } finally {
      setStatsLoading(false);
    }
  };

  const handleViewModeChange = (value: string | null) => {
    if (value) updateSetting('gridViewMode', value as 'waterfall' | 'grid' | 'justified');
  };

  const handleTargetSizeChange = (value: string | number) => {
    const n = Number(value);
    if (Number.isFinite(n) && n >= 100 && n <= 600) {
      updateSetting('gridTargetSize', n);
    }
  };

  return (
    <>
      {/* Appearance */}
      <SettingsBlock title="Appearance">
        <SettingsRow label="Theme">
          <div className={styles.themesPicker}>
            {THEMES.map((t) => (
              <button
                key={t.css}
                className={`${styles.themeSwatch} ${t.css === 'auto' ? styles.themeAuto : ''} ${activeTheme === t.css ? styles.themeActive : ''}`}
                style={t.color ? { backgroundColor: t.color } : undefined}
                title={t.name}
                tabIndex={-1}
                onClick={() => handleThemeChange(t.css)}
              />
            ))}
          </div>
        </SettingsRow>

        <div className={styles.blockSeparator} />

        {/* Language + Zoom side by side */}
        <div className={styles.labelItems}>
          <div className={styles.labelItem}>
            <label>Language</label>
            <div className={styles.right}>
              <Select size="xs" w={110} value="en" data={[{ value: 'en', label: 'English' }]} allowDeselect={false} />
            </div>
          </div>
          <div className={styles.labelItemsSeparator} />
          <div className={styles.labelItem}>
            <label>Zoom</label>
            <div className={styles.right}>
              <Select size="xs" w={80} value={zoom} onChange={handleZoomChange} data={ZOOM_OPTIONS} allowDeselect={false} />
            </div>
          </div>
        </div>
        <SettingsRow label="Hide tag namespaces" separator>
          <Switch
            checked={settings.hideTagNamespace}
            onChange={(e) => updateSetting('hideTagNamespace', e.currentTarget.checked)}
          />
        </SettingsRow>
      </SettingsBlock>

      {/* Grid Defaults */}
      <SettingsBlock title="Grid Defaults">
        <SettingsRow label="Default layout">
          <Select
            size="xs" w={120} value={settings.gridViewMode} onChange={handleViewModeChange}
            data={[
              { value: 'waterfall', label: 'Waterfall' },
              { value: 'grid', label: 'Grid' },
              { value: 'justified', label: 'Justified' },
            ]}
            allowDeselect={false}
          />
        </SettingsRow>
        <SettingsRow label="Thumbnail size" separator>
          <NumberInput size="xs" value={settings.gridTargetSize} onChange={handleTargetSizeChange} min={100} max={600} step={25} w={100} />
        </SettingsRow>
        <SettingsRow label="Sort by" separator>
          <Select
            size="xs" w={120} value={settings.gridSortField}
            onChange={(v) => v && updateSetting('gridSortField', v as typeof settings.gridSortField)}
            data={[
              { value: 'date_added', label: 'Date Added' },
              { value: 'size', label: 'File Size' },
              { value: 'rating', label: 'Rating' },
            ]}
            allowDeselect={false}
          />
        </SettingsRow>
        <SettingsRow label="Sort order" separator>
          <Select
            size="xs" w={120} value={settings.gridSortOrder}
            onChange={(v) => v && updateSetting('gridSortOrder', v as typeof settings.gridSortOrder)}
            data={[
              { value: 'asc', label: 'Ascending' },
              { value: 'desc', label: 'Descending' },
            ]}
            allowDeselect={false}
          />
        </SettingsRow>
        <SettingsRow label="Collection view default" separator>
          <Select
            size="xs" w={140} value={settings.stripDefaultFitMode}
            onChange={(v) => v && updateSetting('stripDefaultFitMode', v as 'horizontal' | 'vertical')}
            data={[
              { value: 'horizontal', label: 'Fit Horizontal' },
              { value: 'vertical', label: 'Fit Vertical' },
            ]}
            allowDeselect={false}
          />
        </SettingsRow>
      </SettingsBlock>

      {/* Features */}
      <SettingsBlock title="Features" description="Choose which reverse image search engines appear in the context menu.">
        {([
          { key: 'tineye' as ReverseSearchEngine, label: 'TinEye' },
          { key: 'saucenao' as ReverseSearchEngine, label: 'SauceNAO' },
          { key: 'yandex' as ReverseSearchEngine, label: 'Yandex Images' },
          { key: 'sogou' as ReverseSearchEngine, label: 'Sogou' },
          { key: 'bing' as ReverseSearchEngine, label: 'Bing Visual Search' },
        ]).map((engine, i) => (
          <SettingsRow key={engine.key} label={engine.label} separator={i > 0}>
            <Switch
              size="xs"
              checked={settings.enabledSearchEngines.includes(engine.key)}
              onChange={(e) => {
                const enabled = settings.enabledSearchEngines;
                const next = e.currentTarget.checked
                  ? [...enabled, engine.key]
                  : enabled.filter(k => k !== engine.key);
                updateSetting('enabledSearchEngines', next);
              }}
            />
          </SettingsRow>
        ))}
      </SettingsBlock>

      {/* Storage Usage */}
      <SettingsBlock title="Storage Usage">
        {statsError && <Text size="sm" c="red">{statsError}</Text>}
        {statsLoading && !stats && !statsError && <Loader size="xs" />}
        {!statsError && stats && (
          <>
            <SettingsRow label="Total files">
              <Text size="sm">{stats.total.toLocaleString()}</Text>
            </SettingsRow>
            <SettingsRow label="Active" light>
              <Text size="sm" c="dimmed">{stats.active.toLocaleString()}</Text>
            </SettingsRow>
            <SettingsRow label="Inbox" light>
              <Text size="sm" c="dimmed">{stats.inbox.toLocaleString()}</Text>
            </SettingsRow>
            <SettingsRow label="Trash" light>
              <Text size="sm" c="dimmed">{stats.trash.toLocaleString()}</Text>
            </SettingsRow>
            <SettingsRow label="Total size (DB)" separator>
              <Text size="sm" fw={600}>{formatFileSize(stats.total_size)}</Text>
            </SettingsRow>

            {stats.breakdown && (
              <>
                <SettingsRow label="Images" light>
                  <Text size="sm" c="dimmed">{stats.breakdown.images.toLocaleString()} &middot; {formatFileSize(stats.breakdown.images_size)}</Text>
                </SettingsRow>
                <SettingsRow label="Videos" light>
                  <Text size="sm" c="dimmed">{stats.breakdown.videos.toLocaleString()} &middot; {formatFileSize(stats.breakdown.videos_size)}</Text>
                </SettingsRow>
                {stats.breakdown.audio > 0 && (
                  <SettingsRow label="Audio" light>
                    <Text size="sm" c="dimmed">{stats.breakdown.audio.toLocaleString()} &middot; {formatFileSize(stats.breakdown.audio_size)}</Text>
                  </SettingsRow>
                )}
                {stats.breakdown.other > 0 && (
                  <SettingsRow label="Other" light>
                    <Text size="sm" c="dimmed">{stats.breakdown.other.toLocaleString()} &middot; {formatFileSize(stats.breakdown.other_size)}</Text>
                  </SettingsRow>
                )}
              </>
            )}

            {stats.originals_disk != null && stats.thumbnails_disk != null && (
              <>
                <SettingsRow label="Originals on disk" separator>
                  <Text size="sm">{formatFileSize(stats.originals_disk)}</Text>
                </SettingsRow>
                <SettingsRow label="Thumbnails on disk" light>
                  <Text size="sm" c="dimmed">{formatFileSize(stats.thumbnails_disk)}</Text>
                </SettingsRow>
              </>
            )}
          </>
        )}
        <SettingsButtonRow>
          <TextButton compact onClick={loadStorageStats} disabled={statsLoading}>
            Refresh
          </TextButton>
        </SettingsButtonRow>
      </SettingsBlock>
    </>
  );
}
