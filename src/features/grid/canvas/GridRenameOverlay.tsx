import type { CanonicalEntityGridItem } from '../../../shared/types/canonical';
import { GlassInput } from '../../../shared/ui/GlassInput/GlassInput';
import type { LayoutItem } from '../layout/types';
import { GRID_TILE_RADIUS } from '../gridAppearance';

interface GridRenameOverlayProps {
  index: number;
  item: CanonicalEntityGridItem;
  position: LayoutItem;
  textHeight: number;
  headerHeight: number;
  onCommit?: (index: number, name: string) => void;
  onCancel?: () => void;
}

/** Reuses the shared field without making rename geometry part of the canvas runtime. */
export function GridRenameOverlay({
  index, item, position, textHeight, headerHeight, onCommit, onCancel,
}: GridRenameOverlayProps) {
  const commitOrCancel = (value: string) => {
    const name = value.trim();
    if (name && name !== (item.name ?? '')) onCommit?.(index, name);
    else onCancel?.();
  };

  return (
    <GlassInput
      autoFocus
      defaultValue={item.name ?? ''}
      style={{
        position: 'absolute',
        left: position.x,
        top: position.y + position.h - textHeight + headerHeight,
        width: position.w,
        height: textHeight,
        zIndex: 200,
        textAlign: 'center',
        padding: '0 4px',
        borderRadius: GRID_TILE_RADIUS,
      }}
      onKeyDown={(event) => {
        if (event.key === 'Enter') {
          event.preventDefault();
          onCommit?.(index, event.currentTarget.value.trim());
        } else if (event.key === 'Escape') {
          event.preventDefault();
          onCancel?.();
        }
      }}
      onBlur={(event) => commitOrCancel(event.currentTarget.value)}
    />
  );
}
