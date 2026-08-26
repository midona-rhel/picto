/**
 * Custom SVG icons — Tabler-compatible (stroke-based, optional fill).
 */

import { forwardRef, type SVGProps } from 'react';

type IconRef = SVGSVGElement;
type IconExtraProps = SVGProps<SVGSVGElement> & { size?: number };

/** Simple person bust — big head, small shoulders. */
export const IconPersonSimple = forwardRef<IconRef, IconExtraProps>(
  ({ size = 24, stroke: strokeWidth = 2, color = 'currentColor', className, ...rest }, ref) => (
    <svg ref={ref} xmlns="http://www.w3.org/2000/svg" width={size} height={size}
      viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={strokeWidth}
      strokeLinecap="round" strokeLinejoin="round" className={className} {...rest}>
      <ellipse cx="12" cy="9" rx="6" ry="7.5" />
      <line x1="9.5" y1="8" x2="9.5" y2="9.5" />
      <line x1="14.5" y1="8" x2="14.5" y2="9.5" />
      <path d="M10 12 Q12 13.5 14 12" />
      <path d="M4 24 C4 20 7 17.5 12 17.5 C17 17.5 20 20 20 24" />
    </svg>
  ),
);
IconPersonSimple.displayName = 'IconPersonSimple';

/**
 * Male — spiky anime-style hair.
 * Head outline only draws from ~4 o'clock around the bottom to ~8 o'clock,
 * stopping well before the hair zone. Hair is big chunky spikes on top.
 */
export const IconPersonMale = forwardRef<IconRef, IconExtraProps>(
  ({ size = 24, stroke: strokeWidth = 2, color = 'currentColor', className, ...rest }, ref) => (
    <svg ref={ref} xmlns="http://www.w3.org/2000/svg" width={size} height={size}
      viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={strokeWidth}
      strokeLinecap="round" strokeLinejoin="round" className={className} {...rest}>
      {/* Head — lower portion only (stops well before hair) */}
      <path d="M6.5 9 C6.5 13 8.5 16.5 12 16.5 C15.5 16.5 17.5 13 17.5 9" />
      {/* Hair — big spiky anime bangs, floating above head */}
      <path d="M6 7 L7.5 2 L10 6 L12 0.5 L14 6 L16.5 2 L18 7" />
      {/* Side hair connecting to head */}
      <path d="M6 7 C5.5 8 5.5 9 6.5 9" />
      <path d="M18 7 C18.5 8 18.5 9 17.5 9" />
      {/* Eyes */}
      <line x1="9.5" y1="9.5" x2="9.5" y2="11" />
      <line x1="14.5" y1="9.5" x2="14.5" y2="11" />
      {/* Smile */}
      <path d="M10 13 Q12 14.2 14 13" />
      {/* Shoulders */}
      <path d="M4 24 C4 20 7 17.5 12 17.5 C17 17.5 20 20 20 24" />
    </svg>
  ),
);
IconPersonMale.displayName = 'IconPersonMale';

/**
 * Female — long flowing anime-style hair with middle part.
 * Head outline only draws the chin/jaw area. Hair is two big sweeping
 * curves from center part flowing down past the face.
 */
export const IconPersonFemale = forwardRef<IconRef, IconExtraProps>(
  ({ size = 24, stroke: strokeWidth = 2, color = 'currentColor', className, ...rest }, ref) => (
    <svg ref={ref} xmlns="http://www.w3.org/2000/svg" width={size} height={size}
      viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={strokeWidth}
      strokeLinecap="round" strokeLinejoin="round" className={className} {...rest}>
      {/* Head — chin/jaw only, stops before hair on both sides */}
      <path d="M7 10 C7 14 9 16.5 12 16.5 C15 16.5 17 14 17 10" />
      {/* Hair — middle part, big sweeping strands flowing down */}
      <path d="M12 0 C8 0 5 2.5 4.5 7 L4 12 C4 13 4.5 14 5 14.5" />
      <path d="M12 0 C16 0 19 2.5 19.5 7 L20 12 C20 13 19.5 14 19 14.5" />
      {/* Inner hair edge — connects to head with gap */}
      <path d="M5.5 8 C6 7 6.5 7 7 7.5" />
      <path d="M18.5 8 C18 7 17.5 7 17 7.5" />
      {/* Eyes */}
      <line x1="9.5" y1="9.5" x2="9.5" y2="11" />
      <line x1="14.5" y1="9.5" x2="14.5" y2="11" />
      {/* Smile */}
      <path d="M10 13 Q12 14.2 14 13" />
      {/* Shoulders */}
      <path d="M4 24 C4 20 7 17.5 12 17.5 C17 17.5 20 20 20 24" />
    </svg>
  ),
);
IconPersonFemale.displayName = 'IconPersonFemale';

/** Folder with a + badge in the bottom-right corner (rounded square around the +).
 *  Folder's bottom and right strokes are shortened to avoid intersecting the badge. */
export const IconFolderNewSelection = forwardRef<IconRef, IconExtraProps>(
  ({ size = 24, stroke: strokeWidth = 1.5, color = 'currentColor', className, ...rest }, ref) => (
    <svg ref={ref} xmlns="http://www.w3.org/2000/svg" width={size} height={size}
      viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth={strokeWidth}
      strokeLinecap="round" strokeLinejoin="round" className={className} {...rest}>
      {/* Folder body — bottom shortened 3 units, right shortened 2 units */}
      {/* Top edge with tab + right edge (stops 2 units before badge) */}
      <path d="M2 6 L2 4 C2 3.45 2.45 3 3 3 L9 3 L11 6 L21 6 C21.55 6 22 6.45 22 7 L22 10" />
      {/* Left edge + bottom (stops 3 units before badge) */}
      <path d="M2 6 L2 18 C2 18.55 2.45 19 3 19 L9 19" />
      {/* Badge: rounded square in bottom-right — slightly bigger */}
      <rect x="13" y="13" width="10" height="10" rx="2.5" ry="2.5" />
      {/* Plus inside badge — centered in the 10x10 rect */}
      <line x1="18" y1="15.5" x2="18" y2="20.5" />
      <line x1="15.5" y1="18" x2="20.5" y2="18" />
    </svg>
  ),
);
IconFolderNewSelection.displayName = 'IconFolderNewSelection';
