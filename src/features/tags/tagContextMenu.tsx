import {
  IconBookmark,
  IconBookmarks,
  IconCopy,
  IconSearch,
  IconStar,
  IconStarOff,
  IconX,
} from '@tabler/icons-react';
import type { MenuEntry } from '../../shared/ui/ContextMenu/ContextMenu';
import type { CanonicalNamespaceSummary } from '../../shared/types/canonical';

export interface TagMenuTarget {
  namespace: string;
  subtag: string;
}

export function tagNamespace(namespace: string): string | null {
  const normalized = namespace.trim().toLowerCase();
  return !normalized || normalized === 'general' ? null : normalized;
}

export function tagName(tag: TagMenuTarget): string {
  const namespace = tagNamespace(tag.namespace);
  return namespace ? `${namespace}:${tag.subtag}` : tag.subtag;
}

export function tagNameInGroup(tag: TagMenuTarget, namespace: string | null): string {
  return namespace ? `${namespace}:${tag.subtag}` : tag.subtag;
}

function copyText(value: string): void {
  const clipboard = (window as Window & { picto?: { clipboard?: { writeText?: (text: string) => void } } }).picto?.clipboard;
  if (clipboard?.writeText) clipboard.writeText(value);
  else void navigator.clipboard.writeText(value);
}

export function buildCommonTagContextEntries({
  tag,
  namespaces,
  starred,
  onFilter,
  onStarChange,
  onMoveToGroup,
  onRemove,
}: {
  tag: TagMenuTarget;
  namespaces: CanonicalNamespaceSummary[];
  starred: boolean;
  onFilter: (tag: string) => void;
  onStarChange: (tag: string, starred: boolean) => void;
  onMoveToGroup?: (namespace: string | null) => void;
  onRemove?: () => void;
}): MenuEntry[] {
  const name = tagName(tag);
  const currentNamespace = tagNamespace(tag.namespace);
  const groupTargets = namespaces
    .map((item) => tagNamespace(item.namespace))
    .filter((namespace): namespace is string => namespace !== null && namespace !== currentNamespace)
    .filter((namespace, index, values) => values.indexOf(namespace) === index)
    .sort((left, right) => left.localeCompare(right));
  const entries: MenuEntry[] = [
    {
      label: 'Filter Items with This Tag',
      icon: <IconSearch size={16} />,
      action: () => onFilter(name),
    },
    {
      label: starred ? 'Remove from Starred' : 'Add to Starred',
      icon: starred ? <IconStarOff size={16} /> : <IconStar size={16} />,
      action: () => onStarChange(name, !starred),
    },
  ];

  if (onMoveToGroup && groupTargets.length > 0) {
    entries.push({
      submenu: true,
      label: currentNamespace ? 'Move to Group…' : 'Add to Group…',
      icon: <IconBookmarks size={16} />,
      children: groupTargets.map((namespace) => ({
        label: namespace,
        icon: <IconBookmark size={16} />,
        action: () => onMoveToGroup(namespace),
      })),
    });
  }
  if (onMoveToGroup && currentNamespace) {
    entries.push({
      label: 'Remove from this Group',
      icon: <IconBookmark size={16} />,
      action: () => onMoveToGroup(null),
    });
  }

  entries.push(
    { separator: true },
    {
      label: 'Copy Tag',
      icon: <IconCopy size={16} />,
      action: () => copyText(name),
    },
  );
  if (onRemove) {
    entries.push({
      label: 'Remove Tag',
      icon: <IconX size={16} />,
      action: onRemove,
    });
  }
  return entries;
}
