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

export interface TagMenuTagLike {
  tag_id: number;
  namespace: string;
  subtag: string;
}

export interface BuildTagMenuArgs {
  tag: TagMenuTagLike;
  aliases: TagMenuTagLike[];
  parents: TagMenuTagLike[];
  children: TagMenuTagLike[];
  formatTagDisplay: (ns: string, subtag: string) => string;
  onShowImages: () => void;
  onRename: () => void;
  onMerge: () => void;
  onCopy: () => void | Promise<void>;
  onViewRelations: () => void;
  onNavigateTag: (ns: string, subtag: string) => void;
  onAddAlias: () => void;
  onAddParent: () => void;
  onAddChild: () => void;
  onDelete: () => void | Promise<void>;
}

function buildRelationChildren(
  relationTags: TagMenuTagLike[],
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
  if (entries.length > 0) entries.push({ type: 'separator' });
  entries.push({
    type: 'item',
    label: addLabel,
    icon: <IconPlus size={16} />,
    onClick: onAdd,
  });
  if (entries.length === 0) {
    entries.push({ type: 'item', label: 'None', disabled: true, onClick: () => {} });
  }
  return entries;
}

export function buildTagContextMenu(args: BuildTagMenuArgs): ContextMenuEntry[] {
  const items: ContextMenuEntry[] = [
    {
      type: 'item',
      label: 'Show Items',
      icon: <IconFilter size={16} />,
      onClick: args.onShowImages,
    },
    { type: 'separator' },
    {
      type: 'item',
      label: 'Rename',
      icon: <IconCursorText size={16} />,
      shortcut: 'F2',
      onClick: args.onRename,
    },
    {
      type: 'item',
      label: 'Merge into…',
      icon: <IconGitMerge size={16} />,
      onClick: args.onMerge,
    },
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
      label: 'Aliases',
      icon: <IconArrowsExchange size={16} />,
      children: buildRelationChildren(
        args.aliases,
        args.onNavigateTag,
        'Add alias…',
        args.onAddAlias,
        args.formatTagDisplay,
      ),
    },
    {
      type: 'submenu',
      label: 'Implications',
      icon: <IconArrowUp size={16} />,
      children: buildRelationChildren(
        args.parents,
        args.onNavigateTag,
        'Add implication…',
        args.onAddParent,
        args.formatTagDisplay,
      ),
    },
    {
      type: 'submenu',
      label: 'Implied By',
      icon: <IconArrowDown size={16} />,
      children: buildRelationChildren(
        args.children,
        args.onNavigateTag,
        'Add implied-by…',
        args.onAddChild,
        args.formatTagDisplay,
      ),
    },
    { type: 'separator' },
    {
      type: 'item',
      label: 'Delete',
      icon: <IconTrash size={16} />,
      danger: true,
      onClick: () => { void args.onDelete(); },
    },
  ];
  return items;
}
