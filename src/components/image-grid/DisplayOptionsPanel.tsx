import { useSettingsStore } from '../../stores/settingsStore';
import { useScopedDisplay } from '../../contexts/ScopedDisplayContext';

function ToggleSwitch({ checked, onClick }: { checked: boolean; onClick: () => void }) {
  return (
    <label
      style={{
        position: 'relative',
        display: 'inline-block',
        width: 32,
        height: 20,
        flexShrink: 0,
        cursor: 'pointer',
      }}
      onClick={(e) => { e.stopPropagation(); onClick(); }}
    >
      <span
        style={{
          position: 'absolute',
          inset: 0,
          borderRadius: 10,
          backgroundColor: checked ? 'var(--color-primary)' : 'rgba(255,255,255,0.10)',
          transition: 'background-color 0.2s ease',
        }}
      />
      <span
        style={{
          position: 'absolute',
          bottom: 2,
          left: 2,
          width: 16,
          height: 16,
          borderRadius: '50%',
          backgroundColor: checked ? '#fff' : 'rgba(255,255,255,0.25)',
          transform: checked ? 'translateX(12px)' : 'translateX(0)',
          transition: 'transform 0.2s ease, background-color 0.2s ease',
        }}
      />
    </label>
  );
}

const rowStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  height: 28,
  gap: 24,
  cursor: 'pointer',
};

const labelStyle: React.CSSProperties = {
  flex: 1,
  fontSize: 'var(--font-size-md)',
  color: 'var(--color-text-primary)',
  whiteSpace: 'nowrap',
};

const valueStyle: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  marginLeft: 'auto',
  gap: 8,
};

// Display options panel — label + toggle switch rows.
// Uses scoped display context when available (per-folder prefs),
// falls back to global settings store otherwise.
export function DisplayOptionsPanel() {
  const { settings, updateSetting } = useSettingsStore();
  const scopedCtx = useScopedDisplay();

  // Effective display values: scoped if available, global fallback
  const showTileName = scopedCtx?.displayOptions.showTileName ?? settings.showTileName;
  const showResolution = scopedCtx?.displayOptions.showResolution ?? settings.showResolution;
  const showExtension = scopedCtx?.displayOptions.showExtension ?? settings.showExtension;
  const showExtensionLabel = scopedCtx?.displayOptions.showExtensionLabel ?? settings.showExtensionLabel;
  const thumbnailFitMode = scopedCtx?.displayOptions.thumbnailFitMode ?? settings.thumbnailFitMode;

  const toggle = (key: 'showTileName' | 'showResolution' | 'showExtension' | 'showExtensionLabel', current: boolean) => {
    if (scopedCtx) {
      scopedCtx.onDisplayOptionChange(key, !current);
    } else {
      updateSetting(key, !current);
    }
  };

  const toggleFit = () => {
    const next = thumbnailFitMode === 'contain' ? 'cover' as const : 'contain' as const;
    if (scopedCtx) {
      scopedCtx.onDisplayOptionChange('thumbnailFitMode', next);
    } else {
      updateSetting('thumbnailFitMode', next);
    }
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6, padding: '4px 0' }}>
      <div style={rowStyle} onClick={() => toggle('showTileName', showTileName)}>
        <div style={labelStyle}>Show Name</div>
        <div style={valueStyle}>
          <ToggleSwitch checked={showTileName} onClick={() => toggle('showTileName', showTileName)} />
        </div>
      </div>

      <div style={rowStyle} onClick={() => toggle('showResolution', showResolution)}>
        <div style={labelStyle}>Show resolution</div>
        <div style={valueStyle}>
          <ToggleSwitch checked={showResolution} onClick={() => toggle('showResolution', showResolution)} />
        </div>
      </div>

      <div style={rowStyle} onClick={() => toggle('showExtension', showExtension)}>
        <div style={labelStyle}>Show extension</div>
        <div style={valueStyle}>
          <ToggleSwitch checked={showExtension} onClick={() => toggle('showExtension', showExtension)} />
        </div>
      </div>

      <div style={rowStyle} onClick={() => toggle('showExtensionLabel', showExtensionLabel)}>
        <div style={labelStyle}>Show label</div>
        <div style={valueStyle}>
          <ToggleSwitch checked={showExtensionLabel} onClick={() => toggle('showExtensionLabel', showExtensionLabel)} />
        </div>
      </div>

      <div style={rowStyle} onClick={toggleFit}>
        <div style={labelStyle}>Fit thumbnails</div>
        <div style={valueStyle}>
          <ToggleSwitch
            checked={thumbnailFitMode === 'cover'}
            onClick={toggleFit}
          />
        </div>
      </div>

      <div style={rowStyle} onClick={() => updateSetting('showSubfolders', !settings.showSubfolders)}>
        <div style={labelStyle}>Show subfolders</div>
        <div style={valueStyle}>
          <ToggleSwitch checked={settings.showSubfolders} onClick={() => updateSetting('showSubfolders', !settings.showSubfolders)} />
        </div>
      </div>

      <div style={{ height: 1, background: 'var(--color-border-secondary)', margin: '2px 0' }} />

      <div style={rowStyle} onClick={() => updateSetting('showSidebar', !settings.showSidebar)}>
        <div style={labelStyle}>Show Sidebar</div>
        <div style={valueStyle}>
          <ToggleSwitch checked={settings.showSidebar} onClick={() => updateSetting('showSidebar', !settings.showSidebar)} />
        </div>
      </div>

      <div style={rowStyle} onClick={() => updateSetting('showInspector', !settings.showInspector)}>
        <div style={labelStyle}>Show Inspector</div>
        <div style={valueStyle}>
          <ToggleSwitch checked={settings.showInspector} onClick={() => updateSetting('showInspector', !settings.showInspector)} />
        </div>
      </div>
    </div>
  );
}
