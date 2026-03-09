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
      <div
        style={{
          position: 'absolute',
          inset: 0,
          borderRadius: 4,
          border: '1px solid var(--color-border-secondary)',
          background: 'linear-gradient(180deg, var(--color-border-primary) 0%, var(--color-border-secondary) 100%)',
          paddingTop: 8,
          paddingLeft: 6,
          paddingRight: 6,
          paddingBottom: 6,
        }}
      >
        <div
          style={{
            width: '100%',
            height: '100%',
            borderRadius: 2,
            border: '1px solid var(--color-border-secondary)',
            background: 'var(--color-theme)',
          }}
        />
      </div>
      <div
        style={{
          position: 'absolute',
          inset: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          zIndex: 1,
        }}
      >
        <IconPhoto size={28} stroke={1.2} style={{ color: 'var(--color-text-tertiary)' }} />
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
              {emptyContext === 'folder' && onImportFolder && (
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
