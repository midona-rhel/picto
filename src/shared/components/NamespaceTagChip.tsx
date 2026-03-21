import { type ReactNode } from 'react';
import { Text, ActionIcon } from '@mantine/core';
import { IconX } from '@tabler/icons-react';
import { useComputedColorScheme } from '@mantine/core';
import { namespaceChipStyle, chipStyleFromRgb } from '../lib/namespaceColors';
import { extractNamespace as extractNamespaceFromTag } from '../lib/tagParsing';
import { useSettingsStore } from '../../state/settingsStore';
import { KbdTooltip } from './KbdTooltip';

interface NamespaceTagChipProps {
  tag: string;
  namespace?: string;
  onRemove?: () => void;
  onLabelClick?: () => void;
  icon?: ReactNode;
  colorRgb?: [number, number, number];
  size?: 'sm' | 'md';
}

export function NamespaceTagChip({ tag, namespace, onRemove, onLabelClick, icon, colorRgb, size = 'md' }: NamespaceTagChipProps) {
  const colorScheme = useComputedColorScheme('dark');
  const isDark = colorScheme === 'dark';
  const hideNamespace = useSettingsStore((s) => s.settings.hideTagNamespace);
  const ns = namespace ?? extractNamespaceFromTag(tag);
  const chipStyle = colorRgb
    ? chipStyleFromRgb(colorRgb, isDark)
    : namespaceChipStyle(ns, isDark);

  // Strip namespace prefix from display text when setting is enabled
  let displayText = tag;
  if (hideNamespace && ns && tag.startsWith(ns + ':')) {
    displayText = tag.slice(ns.length + 1);
  }

  const isSm = size === 'sm';

  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        flexWrap: 'nowrap',
        borderRadius: 'var(--mantine-radius-sm)',
        overflow: 'hidden',
        maxWidth: '100%',
        ...chipStyle,
      }}
    >
      {icon && (
        <span style={{ display: 'flex', alignItems: 'center', marginLeft: 6, color: chipStyle.color }}>
          {icon}
        </span>
      )}
      <Text
        size={isSm ? 'xs' : 'sm'}
        style={{
          lineHeight: 1.3,
          padding: isSm
            ? '2px 4px 2px 6px'
            : onRemove ? '4px 6px 4px 8px' : '4px 8px',
          color: chipStyle.color,
          whiteSpace: 'nowrap',
          cursor: onLabelClick ? 'pointer' : undefined,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          minWidth: 0,
        }}
        onClick={onLabelClick}
      >
        {displayText}
      </Text>
      {onRemove && (
        <KbdTooltip label="Remove">
          <ActionIcon
            variant="transparent"
            size={isSm ? 16 : 18}
            radius="xs"
            onClick={onRemove}
            style={{
              color: chipStyle.color,
              marginRight: isSm ? 2 : 3,
            }}
          >
            <IconX size={isSm ? 9 : 10} />
          </ActionIcon>
        </KbdTooltip>
      )}
    </div>
  );
}
