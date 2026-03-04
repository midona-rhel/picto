import React from 'react';
import {
  Text,
  Badge,
  Anchor,
  ScrollArea,
  Divider,
} from '@mantine/core';
import { TextButton } from './ui/TextButton';
import { api } from '#desktop/api';
import { TagChips, TagWithType } from './TagChips';
import { formatFileSize } from '../lib/formatters';
import sidebarStyles from './MetadataSidebar.module.css';

const formatMetadataKey = (key: string): string =>
  key.replace(/_/g, ' ').replace(/\b\w/g, c => c.toUpperCase());

const SOURCE_DISPLAY_NAMES: Record<string, string> = {
  twitter: 'Twitter',
  fur_affinity: 'Fur Affinity',
  gelbooru: 'Gelbooru',
  e621: 'e621',
};

/** Render text with embedded URLs as clickable links */
function TextWithLinks({ text }: { text: string }) {
  const parts = text.split(/(https?:\/\/[^\s)<>]+)/g);

  return (
    <>
      {parts.map((part, i) =>
        /^https?:\/\//.test(part) ? (
          <Anchor
            key={i}
            size="xs"
            onClick={(e: React.MouseEvent) => {
              e.preventDefault();
              void api.os.openExternalUrl(part);
            }}
            style={{ cursor: 'pointer' }}
          >
            {part}
          </Anchor>
        ) : (
          <React.Fragment key={i}>{part}</React.Fragment>
        )
      )}
    </>
  );
}

const toDisplaySource = (value?: string): string | undefined => {
  if (!value) return undefined;
  const normalized = value.trim().toLowerCase();
  if (normalized === 'gelbooru') return 'Gelbooru';
  if (normalized === 'e621') return 'e621';
  if (normalized === 'affinity') return 'Fur Affinity';
  return value;
};

interface MetadataSidebarProps {
  /** Image URL for blurred background */
  backgroundImageUrl?: string;
  /** Title or filename */
  title: string;
  /** Source badge label (e.g. "gelbooru") */
  source?: string;
  /** Dimensions */
  width: number;
  height: number;
  /** File size in bytes */
  fileSize: number;
  /** Author name */
  author?: string;
  /** Link to original source */
  sourceUrl?: string;
  /** Numeric content rating (source-specific) */
  rating?: number;
  /** Description text */
  description?: string;
  /** Tags with type info for color-coded chips */
  tags: TagWithType[];
  /** Tag editing callbacks */
  onRemoveTag?: (tagName: string) => void;
  onAddTag?: (tagName: string) => void;
  onAddTagWithType?: (tagName: string, tagType: string) => void;
  editable?: boolean;
  /** Save button for dirty tags */
  tagsDirty?: boolean;
  saving?: boolean;
  onSaveTags?: () => void;
  /** Additional metadata key-value pairs */
  additionalMetadata?: Record<string, string>;
  /** Hint text at the bottom */
  hintText?: string;
  /** Extra top padding for content (e.g. to clear a progress bar) */
  contentPaddingTop?: string;
  /** Animation styles for the outer container */
  style?: React.CSSProperties;
  /** Image hash for file operations */
  imageHash?: string;
  /** Collection suggestions for review items */
  collectionSuggestions?: { id: number; name: string; reason: string }[];
  /** Currently selected collection ID */
  selectedCollectionId?: number | null;
  /** Callback when collection selection changes */
  onCollectionSelectionChange?: (collectionId: number | null) => void;
}

export function MetadataSidebar({
  backgroundImageUrl,
  title,
  source,
  width,
  height,
  fileSize,
  author,
  sourceUrl,
  description,
  tags,
  onRemoveTag,
  onAddTag,
  onAddTagWithType: _onAddTagWithType,
  editable = false,
  tagsDirty,
  saving,
  onSaveTags,
  additionalMetadata,
  hintText,
  contentPaddingTop,
  style,
  imageHash: _imageHash,
  collectionSuggestions = [],
  selectedCollectionId,
  onCollectionSelectionChange,
}: MetadataSidebarProps) {
  const handleOpenSource = async (url: string) => {
    try {
      await api.os.openExternalUrl(url);
    } catch (error) {
      console.error('Failed to open source URL:', error);
    }
  };

  const sourceDisplay = toDisplaySource(source);
  const engagementLabel = additionalMetadata?.engagement_label;
  const engagementValue = additionalMetadata?.engagement_value;
  const normalizedSource = source?.trim().toLowerCase();
  const fallbackEngagementLabel =
    normalizedSource === 'gelbooru'
      ? 'Likes'
      : normalizedSource === 'affinity'
        ? 'Stars'
        : undefined;
  const displayEngagementLabel = engagementLabel || fallbackEngagementLabel;
  // Separate comment:* keys from other metadata
  const comments: { source: string | null; text: string }[] = [];
  const metadataEntries: [string, string][] = [];
  if (additionalMetadata) {
    for (const [key, value] of Object.entries(additionalMetadata)) {
      if (key.startsWith('engagement_')) continue;
      if (key === 'comment') {
        comments.push({ source: null, text: value });
      } else if (key.startsWith('comment:')) {
        const sourceKey = key.slice('comment:'.length);
        comments.push({ source: sourceKey, text: value });
      } else {
        metadataEntries.push([key, value]);
      }
    }
  }

  return (
    <div className={sidebarStyles.root} style={{ width: 280, flexShrink: 0, ...style }}>
      {backgroundImageUrl && (
        <>
          <img src={backgroundImageUrl} alt="" className={sidebarStyles.bgFull} />
          <img src={backgroundImageUrl} alt="" className={sidebarStyles.bgTop} />
          <img src={backgroundImageUrl} alt="" className={sidebarStyles.bgBottom} />
        </>
      )}
      <div className={sidebarStyles.darkOverlay} />

      <ScrollArea style={{ flex: 1, minHeight: 0, position: 'relative', zIndex: 1 }} offsetScrollbars>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8, padding: '8px 12px', paddingTop: contentPaddingTop || 8 }}>
          <Text fw={600} size="md" lineClamp={2} c="white">
            {title}
          </Text>

          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            {sourceDisplay && <Badge variant="light">{sourceDisplay}</Badge>}
            <Text size="xs" c="dimmed">{width}x{height}</Text>
            <Text size="xs" c="dimmed">{formatFileSize(fileSize)}</Text>
          </div>

          {sourceDisplay && displayEngagementLabel && engagementValue && (
            <Text size="sm" c="dimmed">
              {sourceDisplay} {displayEngagementLabel}: {engagementValue}
            </Text>
          )}

          {author && (
            <Text size="sm" c="dimmed">by {author}</Text>
          )}

          {sourceUrl && (
            <Anchor
              href={sourceUrl}
              target="_blank"
              rel="noopener noreferrer"
              size="sm"
              onClick={(event) => {
                event.preventDefault();
                void handleOpenSource(sourceUrl);
              }}
            >
              View source
            </Anchor>
          )}

          {description && (
            <Text size="sm" c="dimmed" style={{ wordBreak: 'break-word', overflowWrap: 'break-word' }}>
              <TextWithLinks text={description} />
            </Text>
          )}

          {comments.length > 0 && (
            <>
              <Divider color="var(--ig-border-light)" />
              <div>
                <Text fw={500} size="sm" mb={4} c="dimmed">
                  {comments.length === 1 ? 'Comment' : 'Comments'}
                </Text>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {comments.map((comment, i) => (
                    <div key={i}>
                      {comment.source && (
                        <Badge variant="light" size="xs" mb={2}>
                          {SOURCE_DISPLAY_NAMES[comment.source] || comment.source}
                        </Badge>
                      )}
                      <Text size="xs" c="dimmed" style={{ wordBreak: 'break-word', overflowWrap: 'break-word' }}>
                        <TextWithLinks text={comment.text} />
                      </Text>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}

          <Divider color="var(--ig-border-light)" />

          <div style={{ overflow: 'hidden' }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
              <Text fw={500} size="sm" c="dimmed">Tags</Text>
            </div>
            <TagChips
              tags={tags}
              onRemove={onRemoveTag}
              onAdd={onAddTag}
              editable={editable}
            />
          </div>

          {tagsDirty && onSaveTags && (
            <TextButton onClick={onSaveTags} disabled={saving}>
              {saving ? 'Saving...' : 'Save Tags'}
            </TextButton>
          )}

          {collectionSuggestions.length > 0 && (
            <>
              <Divider color="var(--ig-border-light)" />
              <div>
                <Text fw={500} size="sm" mb={4} c="dimmed">Collection Suggestions</Text>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                  {collectionSuggestions.map((suggestion) => (
                    <div
                      key={suggestion.id}
                      style={{
                        padding: '6px 8px',
                        borderRadius: '4px',
                        border: selectedCollectionId === suggestion.id
                          ? '1px solid rgba(66, 153, 225, 0.6)'
                          : '1px solid var(--ig-border-light)',
                        backgroundColor: selectedCollectionId === suggestion.id
                          ? 'rgba(66, 153, 225, 0.1)'
                          : 'rgba(255,255,255,0.05)',
                        cursor: 'pointer',
                        transition: 'all 0.15s ease',
                      }}
                      onClick={() => {
                        const newId = selectedCollectionId === suggestion.id ? null : suggestion.id;
                        onCollectionSelectionChange?.(newId);
                      }}
                    >
                      <Text size="sm" fw={500} c="white" mb={2}>
                        {suggestion.name}
                      </Text>
                      <Text size="xs" c="dimmed">
                        {suggestion.reason}
                      </Text>
                    </div>
                  ))}
                  <Text size="xs" c="dimmed" style={{ fontStyle: 'italic' }}>
                    Click to select/deselect for auto-assignment
                  </Text>
                </div>
              </div>
            </>
          )}

          {metadataEntries.length > 0 && (
            <>
              <Divider color="var(--ig-border-light)" />
              <div>
                <Text fw={500} size="sm" mb={4} c="dimmed">Metadata</Text>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                  {metadataEntries.map(([key, value]) => (
                    <Text key={key} size="xs" c="dimmed" style={{ wordBreak: 'break-word', overflowWrap: 'break-word' }}>
                      <strong>{formatMetadataKey(key)}:</strong>{' '}
                      <TextWithLinks text={value} />
                    </Text>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      </ScrollArea>

      {hintText && (
        <Text size="xs" c="dimmed" ta="center" py={4} style={{ flexShrink: 0, position: 'relative', zIndex: 1 }}>
          {hintText}
        </Text>
      )}
    </div>
  );
}
