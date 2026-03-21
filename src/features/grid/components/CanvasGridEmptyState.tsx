import { IconFolderPlus, IconPhoto, IconUpload } from '@tabler/icons-react';
import { TextButton } from '../../../shared/components/TextButton';
import { StateActions, StateBlock } from '../../../shared/components/state';
import {
  getEmptyStateDescription,
  getEmptyStateTitle,
} from '../renderer/canvasGridPrimitives';
import type { GridEmptyContext } from '../runtime';

export function CanvasGridEmptyState(props: {
  emptyContext: GridEmptyContext;
  searchTags?: string[];
  onImport: () => void;
  onImportFolder?: () => void;
}) {
  const { emptyContext, searchTags, onImport, onImportFolder } = props;
  const hasSearchTags = !!searchTags?.length;
  const title = getEmptyStateTitle(emptyContext, hasSearchTags);
  const description = getEmptyStateDescription(emptyContext, hasSearchTags);
  const showImportActions =
    emptyContext !== 'inbox' &&
    emptyContext !== 'untagged' &&
    emptyContext !== 'smart-folder' &&
    !hasSearchTags;

  const iconNode = (
    <div
      style={{
        position: 'relative',
        width: 90,
        height: 120,
        marginBottom: -40,
        maskImage: 'linear-gradient(to bottom, black 30%, transparent 100%)',
        WebkitMaskImage: 'linear-gradient(to bottom, black 30%, transparent 100%)',
      }}
    >
      {/* Portrait frame — glass material */}
      <div
        style={{
          position: 'absolute',
          inset: 0,
          borderRadius: 6,
          background: 'rgba(128, 128, 128, 0.12)',
          borderTop: '1px solid rgba(255, 255, 255, 0.25)',
          borderLeft: '1px solid rgba(128, 128, 128, 0.18)',
          borderRight: '1px solid rgba(128, 128, 128, 0.18)',
          borderBottom: '1px solid rgba(0, 0, 0, 0.15)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        {/* Image area */}
        <div
          style={{
            flex: 1,
            margin: 6,
            borderRadius: 3,
            borderTop: '1px solid rgba(255, 255, 255, 0.15)',
            borderLeft: '1px solid rgba(128, 128, 128, 0.12)',
            borderRight: '1px solid rgba(128, 128, 128, 0.12)',
            borderBottom: '1px solid rgba(0, 0, 0, 0.10)',
            background: 'rgba(128, 128, 128, 0.06)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <IconPhoto size={28} stroke={1.2} style={{ color: 'var(--mantine-color-body, #1a1a1e)', opacity: 1 }} />
        </div>
      </div>
    </div>
  );

  return (
    <div style={{ position: 'relative', minHeight: 400 }}>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          textAlign: 'center',
          padding: 40,
          boxSizing: 'border-box',
          WebkitFontSmoothing: 'antialiased',
        }}
      >
        <StateBlock
          variant="empty"
          iconNode={iconNode}
          title={title}
          description={description}
          action={showImportActions ? (
            <StateActions>
              <TextButton onClick={onImport}>
                <IconUpload size={14} />
                Import Files
              </TextButton>
              {onImportFolder && (
                <TextButton onClick={onImportFolder}>
                  <IconFolderPlus size={14} />
                  Import Folder
                </TextButton>
              )}
            </StateActions>
          ) : null}
        />
      </div>
    </div>
  );
}
