import {
  IconBook,
  IconBookmark,
  IconPaw,
  IconPencil,
  IconSettings,
  IconStar,
  IconUser,
  type Icon,
} from '@tabler/icons-react';

type GroupPresentation = {
  color: [number, number, number];
  icon: Icon;
  order: number;
};

const DEFAULT_GROUP: GroupPresentation = {
  color: [114, 160, 193],
  icon: IconBookmark,
  order: 10,
};

const GROUPS: Record<string, GroupPresentation> = {
  creator: { color: [170, 0, 0], icon: IconPencil, order: 0 },
  series: { color: [170, 0, 170], icon: IconBook, order: 1 },
  character: { color: [0, 170, 0], icon: IconUser, order: 2 },
  species: { color: [0, 130, 170], icon: IconPaw, order: 3 },
  rating: { color: [218, 165, 32], icon: IconStar, order: 4 },
  meta: { color: [160, 160, 160], icon: IconSettings, order: 5 },
  general: DEFAULT_GROUP,
  '': DEFAULT_GROUP,
};

export function tagGroupPresentation(namespace: string | null | undefined): GroupPresentation {
  return GROUPS[(namespace ?? '').toLowerCase()] ?? DEFAULT_GROUP;
}

export function tagGroupColor(namespace: string | null | undefined): string {
  const [red, green, blue] = tagGroupPresentation(namespace).color;
  return `rgb(${red}, ${green}, ${blue})`;
}

export function tagGroupOrder(namespace: string): number {
  return tagGroupPresentation(namespace).order;
}
