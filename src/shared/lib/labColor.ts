import type { LabColor } from '../types/canonical';

export function hexToLab(value: string): LabColor {
  const hex = value.replace(/^#/, '');
  const normalized = hex.length === 3
    ? hex.split('').map((part) => `${part}${part}`).join('')
    : hex.padEnd(6, '0').slice(0, 6);
  const rgb = [0, 2, 4]
    .map((offset) => Number.parseInt(normalized.slice(offset, offset + 2), 16) / 255)
    .map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  const x = (rgb[0] * 0.4124 + rgb[1] * 0.3576 + rgb[2] * 0.1805) / 0.95047;
  const y = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
  const z = (rgb[0] * 0.0193 + rgb[1] * 0.1192 + rgb[2] * 0.9505) / 1.08883;
  const convert = (part: number) => part > 0.008856 ? Math.cbrt(part) : 7.787 * part + 16 / 116;
  const fx = convert(x);
  const fy = convert(y);
  const fz = convert(z);
  return { l: 116 * fy - 16, a: 500 * (fx - fy), b: 200 * (fy - fz), weight: 1 };
}

export function labToHex(color: LabColor | null | undefined): string | null {
  if (!color) return null;
  const fy = (color.l + 16) / 116;
  const fx = color.a / 500 + fy;
  const fz = fy - color.b / 200;
  const inverse = (part: number) => part ** 3 > 0.008856 ? part ** 3 : (part - 16 / 116) / 7.787;
  const x = 0.95047 * inverse(fx);
  const y = inverse(fy);
  const z = 1.08883 * inverse(fz);
  const channels = [
    x * 3.2406 + y * -1.5372 + z * -0.4986,
    x * -0.9689 + y * 1.8758 + z * 0.0415,
    x * 0.0557 + y * -0.204 + z * 1.057,
  ].map((channel) => {
    const encoded = channel <= 0.0031308 ? channel * 12.92 : 1.055 * channel ** (1 / 2.4) - 0.055;
    return Math.round(Math.max(0, Math.min(1, encoded)) * 255);
  });
  return `#${channels.map((channel) => channel.toString(16).padStart(2, '0')).join('')}`;
}
