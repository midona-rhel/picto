/**
 * Outline SVG icons for reverse-image-search engines.
 * All paths use `currentColor` so they inherit the context-menu text colour.
 */

interface IconProps {
  size?: number;
}

/** TinEye — stylised eye with circuit lines */
export function IconTinEye({ size = 16 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {/* Outer eye shape */}
      <path d="M2 12C2 12 5.5 5 12 5C18.5 5 22 12 22 12C22 12 18.5 19 12 19C5.5 19 2 12 2 12Z" />
      {/* Iris */}
      <circle cx="12" cy="12" r="3.5" />
      {/* Pupil dot */}
      <circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** SauceNAO — bowl/dish shape (sauce reference) with magnifier */
export function IconSauceNAO({ size = 16 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {/* Magnifying glass */}
      <circle cx="10.5" cy="10.5" r="6" />
      <line x1="15" y1="15" x2="21" y2="21" />
      {/* Inner image indicator — small mountain/landscape */}
      <path d="M7.5 13L9.5 10.5L11 12L13 9.5L15 13" strokeWidth="1.2" />
    </svg>
  );
}

/** Yandex — Y letterform in a circle */
export function IconYandex({ size = 16 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="9.5" />
      {/* Y letterform */}
      <path d="M9 6L12 12M15 6L12 12M12 12V18" />
    </svg>
  );
}

/** Sogou — S letterform in a rounded square */
export function IconSogou({ size = 16 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="18" height="18" rx="4" />
      {/* S letterform */}
      <path d="M15 8.5C15 8.5 14 7 12 7C10 7 9 8.2 9 9.5C9 11 10.5 11.5 12 12C13.5 12.5 15 13 15 14.5C15 15.8 14 17 12 17C10 17 9 15.5 9 15.5" />
    </svg>
  );
}

/** Bing — B letterform with a lens flare accent */
export function IconBing({ size = 16 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {/* b letterform — Bing logo style */}
      <path d="M7 3V17L11 19L17 15.5V12.5L11 10L7 11.5" />
      <path d="M7 11.5L11 10V5L7 3" />
    </svg>
  );
}
