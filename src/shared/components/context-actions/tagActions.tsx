import {
  IconArrowsExchange,
  IconArrowDown,
  IconArrowUp,
  IconCopy,
  IconCursorText,
  IconFilter,
  IconGitMerge,
  IconHierarchy2,
  IconPlus,
  IconTrash,
} from '@tabler/icons-react';
import type { ContextMenuEntry } from '../ContextMenu';

export type TagSourceKind = 'local' | 'ptr';

export interface TagMenuTagLike {
  tag_id: number;
  namespace: string;
  subtag: string;
}

export interface BuildTagMenuArgs {
  tag: TagMenuTagLike;
  source: TagSourceKind;
  siblings: TagMenuTagLike[];
  parents: TagMenuTagLike[];
  children: TagMenuTagLike[];
  formatTagDisplay: (ns: string, subtag: string) => string;
  onShowImages: () => void;
  onRename: () => void;
  onMerge: () => void;
  onCopy: () => void | Promise<void>;
  onViewRelations: () => void;
  onNavigateTag: (ns: string, subtag: string) => void;
  onAddSibling: () => void;
  onAddParent: () => void;
  onAddChild: () => void;
  onDelete: () => void | Promise<void>;
}

function buildRelationChildren(
  relationTags: TagMenuTagLike[],
  isPtr: boolean,
  onNavigateTag: (ns: string, subtag: string) => void,
  addLabel: string,
  onAdd: () => void,
  formatTagDisplay: (ns: string, subtag: string) => string,
): ContextMenuEntry[] {
  const entries: ContextMenuEntry[] = relationTags.map((t) => ({
    type: 'item',
    label: formatTagDisplay(t.namespace, t.subtag),
    onClick: () => onNavigateTag(t.namespace, t.subtag),
  }));
  if (!isPtr) {
    if (entries.length > 0) entries.push({ type: 'separator' });
    entries.push({
      type: 'item',
      label: addLabel,
      icon: <IconPlus size={16} />,
      onClick: onAdd,
    });
  }
  if (entries.length === 0) {
    entries.push({ type: 'item', label: 'None', disabled: true, onClick: () => {} });
  }
  return entries;
}

export function buildTagContextMenu(args: BuildTagMenuArgs): ContextMenuEntry[] {
  const isPtr = args.source === 'ptr';
  const items: ContextMenuEntry[] = [
    {
      type: 'item',
      label: 'Show Images',
      icon: <IconFilter size={16} />,
      onClick: args.onShowImages,
    },
    { type: 'separator' },
    ...(!isPtr ? [
      {
        type: 'item' as const,
        label: 'Rename',
        icon: <IconCursorText size={16} />,
        shortcut: 'F2',
        onClick: args.onRename,
      },
      {
        type: 'item' as const,
        label: 'Merge into…',
        icon: <IconGitMerge size={16} />,
        onClick: args.onMerge,
      },
    ] : []),
    {
      type: 'item',
      label: 'Copy',
      icon: <IconCopy size={16} />,
      onClick: () => { void args.onCopy(); },
    },
    {
      type: 'item',
      label: 'View Relations',
      icon: <IconHierarchy2 size={16} />,
      onClick: args.onViewRelations,
    },
    { type: 'separator' },
    {
      type: 'submenu',
      label: 'Siblings',
      icon: <IconArrowsExchange size={16} />,
      children: buildRelationChildren(
        args.siblings,
        isPtr,
        args.onNavigateTag,
        'Add sibling…',
        args.onAddSibling,
        args.formatTagDisplay,
      ),
    },
    {
      type: 'submenu',
      label: 'Parents',
      icon: <IconArrowUp size={16} />,
      children: buildRelationChildren(
        args.parents,
        isPtr,
        args.onNavigateTag,
        'Add parent…',
        args.onAddParent,
        args.formatTagDisplay,
      ),
    },
    {
      type: 'submenu',
      label: 'Children',
      icon: <IconArrowDown size={16} />,
      children: buildRelationChildren(
        args.children,
        isPtr,
        args.onNavigateTag,
        'Add child…',
        args.onAddChild,
        args.formatTagDisplay,
      ),
    },
    ...(!isPtr ? [
      { type: 'separator' as const },
      {
        type: 'item' as const,
        label: 'Delete',
        icon: <IconTrash size={16} />,
        danger: true,
        onClick: () => { void args.onDelete(); },
      },
    ] : []),
  ];
  return items;
}
