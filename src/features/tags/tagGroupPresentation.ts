import {
  IconBook,
  IconBookmark,
  IconBuilding,
  IconMoodSmile,
  IconPaw,
  IconPhoto,
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
  creator: { color: [170, 0, 0], icon: IconUser, order: 0 },
  studio: { color: [128, 0, 0], icon: IconBuilding, order: 1 },
  series: { color: [170, 0, 170], icon: IconBook, order: 2 },
  character: { color: [0, 170, 0], icon: IconMoodSmile, order: 3 },
  person: { color: [0, 128, 0], icon: IconUser, order: 4 },
  species: { color: [0, 130, 170], icon: IconPaw, order: 5 },
  photoset: { color: [114, 160, 193], icon: IconPhoto, order: 6 },
  rating: { color: [218, 165, 32], icon: IconStar, order: 7 },
  meta: { color: [160, 160, 160], icon: IconSettings, order: 8 },
  system: { color: [153, 101, 21], icon: IconSettings, order: 9 },
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
