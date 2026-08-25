import { useId, type SVGProps } from 'react';
import {
  FONT_THUMBNAIL_CARD,
  FONT_THUMBNAIL_BASELINE,
  FONT_THUMBNAIL_FAMILY,
  FONT_THUMBNAIL_GLYPH_GAP,
  FONT_THUMBNAIL_GLYPHS,
  FONT_THUMBNAIL_SIZE,
} from './fontThumbnailGeometry';

/** Thumbnail used for fonts, which intentionally have no raster thumbnail. */
export function FontThumbnail(props: SVGProps<SVGSVGElement>) {
  const id = useId().split(':').join('');
  const fillId = `font-tile-fill-${id}`;
  const maskId = `font-tile-mask-${id}`;

  return (
    <svg width={FONT_THUMBNAIL_SIZE} height={FONT_THUMBNAIL_SIZE} {...props} viewBox="0 0 160 160" aria-hidden="true" data-font-thumbnail>
      <defs>
        <linearGradient id={fillId} x1="30" y1="12" x2="130" y2="152" gradientUnits="userSpaceOnUse">
          <stop stopColor="#F7F8FA" stopOpacity=".88" />
          <stop offset="1" stopColor="#A7ABB2" stopOpacity=".68" />
        </linearGradient>
        <mask
          id={maskId}
          maskUnits="userSpaceOnUse"
          x={FONT_THUMBNAIL_CARD.x}
          y={FONT_THUMBNAIL_CARD.y}
          width={FONT_THUMBNAIL_CARD.size}
          height={FONT_THUMBNAIL_CARD.size}
          style={{ maskType: 'luminance' }}
        >
          <rect
            x={FONT_THUMBNAIL_CARD.x}
            y={FONT_THUMBNAIL_CARD.y}
            width={FONT_THUMBNAIL_CARD.size}
            height={FONT_THUMBNAIL_CARD.size}
            rx={FONT_THUMBNAIL_CARD.radius}
            fill="white"
          />
          <text
            x={FONT_THUMBNAIL_SIZE / 2}
            y={FONT_THUMBNAIL_BASELINE}
            textAnchor="middle"
            fill="black"
            fontFamily={FONT_THUMBNAIL_FAMILY}
            fontWeight="700"
            style={{ fontKerning: 'normal', fontVariantLigatures: 'none' }}
          >
            {FONT_THUMBNAIL_GLYPHS.map((glyph, index) => (
              <tspan key={glyph.text} fontSize={glyph.size} dx={index === 0 ? 0 : FONT_THUMBNAIL_GLYPH_GAP}>
                {glyph.text}
              </tspan>
            ))}
          </text>
        </mask>
        <filter id={`font-tile-shadow-${id}`} x="0" y="0" width="160" height="160">
          <feDropShadow dx="0" dy="5" stdDeviation="6" floodColor="#000" floodOpacity=".24" />
        </filter>
      </defs>
      <rect
        x={FONT_THUMBNAIL_CARD.x}
        y={FONT_THUMBNAIL_CARD.y}
        width={FONT_THUMBNAIL_CARD.size}
        height={FONT_THUMBNAIL_CARD.size}
        rx={FONT_THUMBNAIL_CARD.radius}
        fill={`url(#${fillId})`}
        mask={`url(#${maskId})`}
        filter={`url(#font-tile-shadow-${id})`}
      />
    </svg>
  );
}
