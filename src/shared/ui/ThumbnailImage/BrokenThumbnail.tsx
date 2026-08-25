import { useId, type SVGProps } from 'react';
import {
  BROKEN_DOCUMENT_BODY_PATH,
  BROKEN_DOCUMENT_CRACK_PATH,
  BROKEN_DOCUMENT_FOLD_PATH,
} from './brokenThumbnailGeometry';

export function BrokenThumbnail(props: SVGProps<SVGSVGElement>) {
  const id = useId().split(':').join('');
  const paper = `broken-paper-${id}`;
  const fold = `broken-fold-${id}`;
  const shadow = `broken-shadow-${id}`;
  return (
    <svg width="160" height="176" {...props} viewBox="0 0 160 176" fill="none" aria-hidden="true" data-broken-thumbnail>
      <defs>
        <linearGradient id={paper} x1="45" y1="18" x2="112" y2="160" gradientUnits="userSpaceOnUse">
          <stop stopColor="#F7F8FA" stopOpacity=".9" />
          <stop offset="1" stopColor="#A7ABB2" stopOpacity=".66" />
        </linearGradient>
        <linearGradient id={fold} x1="108" y1="18" x2="137" y2="47" gradientUnits="userSpaceOnUse">
          <stop stopColor="#ECEEF1" stopOpacity=".86" />
          <stop offset="1" stopColor="#9DA2AA" stopOpacity=".62" />
        </linearGradient>
        <filter id={shadow} x="7" y="3" width="149" height="174" colorInterpolationFilters="sRGB">
          <feDropShadow dx="0" dy="6" stdDeviation="7" floodColor="#000" floodOpacity=".26" />
        </filter>
      </defs>
      <g filter={`url(#${shadow})`}>
        <path d={BROKEN_DOCUMENT_BODY_PATH} fill={`url(#${paper})`} stroke="#FFF" strokeOpacity=".25" />
        <path d="M108 18L137 47H108Z" fill={`url(#${fold})`} />
        <path d={BROKEN_DOCUMENT_FOLD_PATH} stroke="var(--color-surface-1, #27282d)" strokeWidth="11" strokeLinecap="butt" strokeLinejoin="miter" />
        <path d={BROKEN_DOCUMENT_CRACK_PATH} stroke="var(--color-surface-1, #27282d)" strokeWidth="11" strokeLinecap="butt" strokeLinejoin="miter" />
      </g>
    </svg>
  );
}
