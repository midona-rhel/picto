import type { TablerIcon } from '@tabler/icons-react';
import {
  // Media
  IconPhoto, IconVideo, IconMusic, IconCamera, IconPlayerPlay, IconMicrophone,
  IconHeadphones, IconVolume, IconScreenShare, IconBrandYoutube,
  // Files & Documents
  IconFile, IconFileText, IconFolder, IconFolderOpen, IconArchive, IconClipboard,
  IconNote, IconBook, IconNotebook, IconFileZip,
  // Shapes & Abstract
  IconSquare, IconCircle, IconTriangle, IconHexagon, IconDiamond, IconStar,
  IconHeart, IconStarFilled, IconHeartFilled, IconSparkles,
  // Objects & Things
  IconHome, IconBuildingSkyscraper, IconCar, IconPlane, IconRocket, IconAnchor,
  IconCompass, IconMap, IconGlobe, IconFlag,
  // Nature & Weather
  IconSun, IconMoon, IconCloud, IconSnowflake, IconDroplet, IconFlame,
  IconLeaf, IconTree, IconMountain, IconWind,
  // Animals
  IconDog, IconCat, IconFish, IconBug, IconFeather, IconPaw,
  // People & Body
  IconUser, IconUsers, IconUserCircle, IconMoodSmile, IconMoodHappy, IconFriends,
  IconMan, IconWoman, IconAccessible,
  // Communication
  IconMail, IconMessage, IconBell, IconPhone, IconBrandTwitter, IconBrandInstagram,
  IconSend, IconAt, IconHash, IconBrandDiscord,
  // Art & Creative
  IconPalette, IconBrush, IconPencil, IconHighlight, IconScissors, IconRuler,
  IconColorSwatch, IconDropletFilled, IconWand, IconSpray,
  // Tech & Code
  IconCode, IconTerminal, IconDatabase, IconServer, IconCpu, IconWifi,
  IconBluetooth, IconDeviceDesktop, IconDeviceMobile, IconBrandGithub,
  // Misc
  IconTag, IconBookmark, IconPin, IconLock, IconShield, IconEye,
  IconClock, IconCalendar, IconGift, IconTrophy,
  IconCrown, IconBolt, IconTarget, IconPuzzle, IconDice, IconChess,
  IconAward, IconCoffee, IconPizza, IconApple,
} from '@tabler/icons-react';

export interface CuratedIcon {
  name: string;
  label: string;
  component: TablerIcon;
  category: string;
}

export const CURATED_ICONS: CuratedIcon[] = [
  // Media
  { name: 'IconPhoto', label: 'Photo', component: IconPhoto, category: 'Media' },
  { name: 'IconVideo', label: 'Video', component: IconVideo, category: 'Media' },
  { name: 'IconMusic', label: 'Music', component: IconMusic, category: 'Media' },
  { name: 'IconCamera', label: 'Camera', component: IconCamera, category: 'Media' },
  { name: 'IconPlayerPlay', label: 'Play', component: IconPlayerPlay, category: 'Media' },
  { name: 'IconMicrophone', label: 'Microphone', component: IconMicrophone, category: 'Media' },
  { name: 'IconHeadphones', label: 'Headphones', component: IconHeadphones, category: 'Media' },
  { name: 'IconVolume', label: 'Volume', component: IconVolume, category: 'Media' },
  { name: 'IconScreenShare', label: 'Screen', component: IconScreenShare, category: 'Media' },
  { name: 'IconBrandYoutube', label: 'YouTube', component: IconBrandYoutube, category: 'Media' },

  // Files & Documents
  { name: 'IconFile', label: 'File', component: IconFile, category: 'Files' },
  { name: 'IconFileText', label: 'Text File', component: IconFileText, category: 'Files' },
  { name: 'IconFolder', label: 'Folder', component: IconFolder, category: 'Files' },
  { name: 'IconFolderOpen', label: 'Open Folder', component: IconFolderOpen, category: 'Files' },
  { name: 'IconArchive', label: 'Archive', component: IconArchive, category: 'Files' },
  { name: 'IconClipboard', label: 'Clipboard', component: IconClipboard, category: 'Files' },
  { name: 'IconNote', label: 'Note', component: IconNote, category: 'Files' },
  { name: 'IconBook', label: 'Book', component: IconBook, category: 'Files' },
  { name: 'IconNotebook', label: 'Notebook', component: IconNotebook, category: 'Files' },
  { name: 'IconFileZip', label: 'Zip', component: IconFileZip, category: 'Files' },

  // Shapes & Abstract
  { name: 'IconSquare', label: 'Square', component: IconSquare, category: 'Shapes' },
  { name: 'IconCircle', label: 'Circle', component: IconCircle, category: 'Shapes' },
  { name: 'IconTriangle', label: 'Triangle', component: IconTriangle, category: 'Shapes' },
  { name: 'IconHexagon', label: 'Hexagon', component: IconHexagon, category: 'Shapes' },
  { name: 'IconDiamond', label: 'Diamond', component: IconDiamond, category: 'Shapes' },
  { name: 'IconStar', label: 'Star', component: IconStar, category: 'Shapes' },
  { name: 'IconHeart', label: 'Heart', component: IconHeart, category: 'Shapes' },
  { name: 'IconStarFilled', label: 'Star Filled', component: IconStarFilled, category: 'Shapes' },
  { name: 'IconHeartFilled', label: 'Heart Filled', component: IconHeartFilled, category: 'Shapes' },
  { name: 'IconSparkles', label: 'Sparkles', component: IconSparkles, category: 'Shapes' },

  // Objects & Things
  { name: 'IconHome', label: 'Home', component: IconHome, category: 'Objects' },
  { name: 'IconBuildingSkyscraper', label: 'Building', component: IconBuildingSkyscraper, category: 'Objects' },
  { name: 'IconCar', label: 'Car', component: IconCar, category: 'Objects' },
  { name: 'IconPlane', label: 'Plane', component: IconPlane, category: 'Objects' },
  { name: 'IconRocket', label: 'Rocket', component: IconRocket, category: 'Objects' },
  { name: 'IconAnchor', label: 'Anchor', component: IconAnchor, category: 'Objects' },
  { name: 'IconCompass', label: 'Compass', component: IconCompass, category: 'Objects' },
  { name: 'IconMap', label: 'Map', component: IconMap, category: 'Objects' },
  { name: 'IconGlobe', label: 'Globe', component: IconGlobe, category: 'Objects' },
  { name: 'IconFlag', label: 'Flag', component: IconFlag, category: 'Objects' },

  // Nature & Weather
  { name: 'IconSun', label: 'Sun', component: IconSun, category: 'Nature' },
  { name: 'IconMoon', label: 'Moon', component: IconMoon, category: 'Nature' },
  { name: 'IconCloud', label: 'Cloud', component: IconCloud, category: 'Nature' },
  { name: 'IconSnowflake', label: 'Snowflake', component: IconSnowflake, category: 'Nature' },
  { name: 'IconDroplet', label: 'Droplet', component: IconDroplet, category: 'Nature' },
  { name: 'IconFlame', label: 'Flame', component: IconFlame, category: 'Nature' },
  { name: 'IconLeaf', label: 'Leaf', component: IconLeaf, category: 'Nature' },
  { name: 'IconTree', label: 'Tree', component: IconTree, category: 'Nature' },
  { name: 'IconMountain', label: 'Mountain', component: IconMountain, category: 'Nature' },
  { name: 'IconWind', label: 'Wind', component: IconWind, category: 'Nature' },

  // Animals
  { name: 'IconDog', label: 'Dog', component: IconDog, category: 'Animals' },
  { name: 'IconCat', label: 'Cat', component: IconCat, category: 'Animals' },
  { name: 'IconFish', label: 'Fish', component: IconFish, category: 'Animals' },
  { name: 'IconBug', label: 'Bug', component: IconBug, category: 'Animals' },
  { name: 'IconFeather', label: 'Feather', component: IconFeather, category: 'Animals' },
  { name: 'IconPaw', label: 'Paw', component: IconPaw, category: 'Animals' },

  // People
  { name: 'IconUser', label: 'User', component: IconUser, category: 'People' },
  { name: 'IconUsers', label: 'Users', component: IconUsers, category: 'People' },
  { name: 'IconUserCircle', label: 'User Circle', component: IconUserCircle, category: 'People' },
  { name: 'IconMoodSmile', label: 'Smile', component: IconMoodSmile, category: 'People' },
  { name: 'IconMoodHappy', label: 'Happy', component: IconMoodHappy, category: 'People' },
  { name: 'IconFriends', label: 'Friends', component: IconFriends, category: 'People' },
  { name: 'IconMan', label: 'Man', component: IconMan, category: 'People' },
  { name: 'IconWoman', label: 'Woman', component: IconWoman, category: 'People' },
  { name: 'IconAccessible', label: 'Accessible', component: IconAccessible, category: 'People' },

  // Communication
  { name: 'IconMail', label: 'Mail', component: IconMail, category: 'Communication' },
  { name: 'IconMessage', label: 'Message', component: IconMessage, category: 'Communication' },
  { name: 'IconBell', label: 'Bell', component: IconBell, category: 'Communication' },
  { name: 'IconPhone', label: 'Phone', component: IconPhone, category: 'Communication' },
  { name: 'IconBrandTwitter', label: 'Twitter', component: IconBrandTwitter, category: 'Communication' },
  { name: 'IconBrandInstagram', label: 'Instagram', component: IconBrandInstagram, category: 'Communication' },
  { name: 'IconSend', label: 'Send', component: IconSend, category: 'Communication' },
  { name: 'IconAt', label: 'At', component: IconAt, category: 'Communication' },
  { name: 'IconHash', label: 'Hash', component: IconHash, category: 'Communication' },
  { name: 'IconBrandDiscord', label: 'Discord', component: IconBrandDiscord, category: 'Communication' },

  // Art & Creative
  { name: 'IconPalette', label: 'Palette', component: IconPalette, category: 'Art' },
  { name: 'IconBrush', label: 'Brush', component: IconBrush, category: 'Art' },
  { name: 'IconPencil', label: 'Pencil', component: IconPencil, category: 'Art' },
  { name: 'IconHighlight', label: 'Highlight', component: IconHighlight, category: 'Art' },
  { name: 'IconScissors', label: 'Scissors', component: IconScissors, category: 'Art' },
  { name: 'IconRuler', label: 'Ruler', component: IconRuler, category: 'Art' },
  { name: 'IconColorSwatch', label: 'Color Swatch', component: IconColorSwatch, category: 'Art' },
  { name: 'IconDropletFilled', label: 'Ink Drop', component: IconDropletFilled, category: 'Art' },
  { name: 'IconWand', label: 'Wand', component: IconWand, category: 'Art' },
  { name: 'IconSpray', label: 'Spray', component: IconSpray, category: 'Art' },

  // Tech & Code
  { name: 'IconCode', label: 'Code', component: IconCode, category: 'Tech' },
  { name: 'IconTerminal', label: 'Terminal', component: IconTerminal, category: 'Tech' },
  { name: 'IconDatabase', label: 'Database', component: IconDatabase, category: 'Tech' },
  { name: 'IconServer', label: 'Server', component: IconServer, category: 'Tech' },
  { name: 'IconCpu', label: 'CPU', component: IconCpu, category: 'Tech' },
  { name: 'IconWifi', label: 'WiFi', component: IconWifi, category: 'Tech' },
  { name: 'IconBluetooth', label: 'Bluetooth', component: IconBluetooth, category: 'Tech' },
  { name: 'IconDeviceDesktop', label: 'Desktop', component: IconDeviceDesktop, category: 'Tech' },
  { name: 'IconDeviceMobile', label: 'Mobile', component: IconDeviceMobile, category: 'Tech' },
  { name: 'IconBrandGithub', label: 'GitHub', component: IconBrandGithub, category: 'Tech' },

  // Misc
  { name: 'IconTag', label: 'Tag', component: IconTag, category: 'Misc' },
  { name: 'IconBookmark', label: 'Bookmark', component: IconBookmark, category: 'Misc' },
  { name: 'IconPin', label: 'Pin', component: IconPin, category: 'Misc' },
  { name: 'IconLock', label: 'Lock', component: IconLock, category: 'Misc' },
  { name: 'IconShield', label: 'Shield', component: IconShield, category: 'Misc' },
  { name: 'IconEye', label: 'Eye', component: IconEye, category: 'Misc' },
  { name: 'IconClock', label: 'Clock', component: IconClock, category: 'Misc' },
  { name: 'IconCalendar', label: 'Calendar', component: IconCalendar, category: 'Misc' },
  { name: 'IconGift', label: 'Gift', component: IconGift, category: 'Misc' },
  { name: 'IconTrophy', label: 'Trophy', component: IconTrophy, category: 'Misc' },
  { name: 'IconCrown', label: 'Crown', component: IconCrown, category: 'Misc' },
  { name: 'IconBolt', label: 'Bolt', component: IconBolt, category: 'Misc' },
  { name: 'IconTarget', label: 'Target', component: IconTarget, category: 'Misc' },
  { name: 'IconPuzzle', label: 'Puzzle', component: IconPuzzle, category: 'Misc' },
  { name: 'IconDice', label: 'Dice', component: IconDice, category: 'Misc' },
  { name: 'IconChess', label: 'Chess', component: IconChess, category: 'Misc' },
  { name: 'IconAward', label: 'Award', component: IconAward, category: 'Misc' },
  { name: 'IconCoffee', label: 'Coffee', component: IconCoffee, category: 'Misc' },
  { name: 'IconPizza', label: 'Pizza', component: IconPizza, category: 'Misc' },
  { name: 'IconApple', label: 'Apple', component: IconApple, category: 'Misc' },
];

export const ICON_MAP = new Map<string, TablerIcon>(
  CURATED_ICONS.map((icon) => [icon.name, icon.component])
);

export const ICON_CATEGORIES = [...new Set(CURATED_ICONS.map((i) => i.category))];

export const DEFAULT_FOLDER_ICON = 'IconFolder';

export function getIconComponent(name: string): TablerIcon | null {
  return ICON_MAP.get(name) ?? null;
}

export function DynamicIcon({
  name,
  size = 16,
  color,
  stroke = 1.5,
}: {
  name: string;
  size?: number;
  color?: string;
  stroke?: number;
}) {
  const Icon = ICON_MAP.get(name);
  if (!Icon) {
    const Fallback = ICON_MAP.get(DEFAULT_FOLDER_ICON)!;
    return <Fallback size={size} color={color} stroke={stroke} />;
  }
  return <Icon size={size} color={color} stroke={stroke} />;
}
