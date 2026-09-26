// Распознавание и конвертация цветов (Pro-фича, DECISIONS §7).
// Разбираем ТОЛЬКО строку, которая целиком является одним цветом
// (HEX / rgb(a) / hsl(a)). Возвращаем нормализованный RGBA (0..255, a: 0..1).

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, n));

function parseHex(s: string): Rgba | null {
  const m = /^#([0-9a-fA-F]{3,8})$/.exec(s);
  if (!m) return null;
  const h = m[1];
  const expand = (c: string) => parseInt(c + c, 16);
  if (h.length === 3 || h.length === 4) {
    return {
      r: expand(h[0]),
      g: expand(h[1]),
      b: expand(h[2]),
      a: h.length === 4 ? expand(h[3]) / 255 : 1,
    };
  }
  if (h.length === 6 || h.length === 8) {
    return {
      r: parseInt(h.slice(0, 2), 16),
      g: parseInt(h.slice(2, 4), 16),
      b: parseInt(h.slice(4, 6), 16),
      a: h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1,
    };
  }
  return null;
}

// Числа, разделённые запятыми или пробелами; альфа — после запятой или "/".
function parseFuncArgs(inside: string): { nums: number[]; alpha: number } {
  const parts = inside.split("/");
  const main = parts[0].trim().split(/[\s,]+/).filter(Boolean);
  let alpha = 1;
  if (parts[1] !== undefined) {
    alpha = parseAlpha(parts[1].trim());
  } else if (main.length === 4) {
    alpha = parseAlpha(main.pop() as string);
  }
  const nums = main.map((x) => parseFloat(x));
  return { nums, alpha };
}

function parseAlpha(s: string): number {
  if (s.endsWith("%")) return clamp(parseFloat(s) / 100, 0, 1);
  return clamp(parseFloat(s), 0, 1);
}

function parseRgb(s: string): Rgba | null {
  const m = /^rgba?\(([^)]+)\)$/i.exec(s);
  if (!m) return null;
  const { nums, alpha } = parseFuncArgs(m[1]);
  if (nums.length < 3 || nums.some((n) => Number.isNaN(n))) return null;
  const conv = (raw: string, v: number) => (raw.includes("%") ? (v / 100) * 255 : v);
  // Проценты внутри rgb() применяем покомпонентно.
  const rawParts = m[1].split("/")[0].trim().split(/[\s,]+/).filter(Boolean);
  return {
    r: clamp(Math.round(conv(rawParts[0], nums[0])), 0, 255),
    g: clamp(Math.round(conv(rawParts[1], nums[1])), 0, 255),
    b: clamp(Math.round(conv(rawParts[2], nums[2])), 0, 255),
    a: alpha,
  };
}

function parseHsl(s: string): Rgba | null {
  const m = /^hsla?\(([^)]+)\)$/i.exec(s);
  if (!m) return null;
  const { nums, alpha } = parseFuncArgs(m[1]);
  if (nums.length < 3 || nums.some((n) => Number.isNaN(n))) return null;
  const h = ((nums[0] % 360) + 360) % 360;
  const sat = clamp(nums[1], 0, 100) / 100;
  const lig = clamp(nums[2], 0, 100) / 100;
  const c = (1 - Math.abs(2 * lig - 1)) * sat;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const mm = lig - c / 2;
  let rp = 0;
  let gp = 0;
  let bp = 0;
  if (h < 60) [rp, gp, bp] = [c, x, 0];
  else if (h < 120) [rp, gp, bp] = [x, c, 0];
  else if (h < 180) [rp, gp, bp] = [0, c, x];
  else if (h < 240) [rp, gp, bp] = [0, x, c];
  else if (h < 300) [rp, gp, bp] = [x, 0, c];
  else [rp, gp, bp] = [c, 0, x];
  return {
    r: Math.round((rp + mm) * 255),
    g: Math.round((gp + mm) * 255),
    b: Math.round((bp + mm) * 255),
    a: alpha,
  };
}

/// Разбирает строку-цвет целиком; иначе null.
export function parseColor(input: string): Rgba | null {
  const s = input.trim();
  if (s.length === 0 || s.length > 32) return null;
  return parseHex(s) || parseRgb(s) || parseHsl(s);
}

const hx = (n: number) => clamp(Math.round(n), 0, 255).toString(16).padStart(2, "0");

export function toHex(c: Rgba): string {
  const base = `#${hx(c.r)}${hx(c.g)}${hx(c.b)}`;
  return c.a < 1 ? base + hx(c.a * 255) : base;
}

export function toRgb(c: Rgba): string {
  return c.a < 1
    ? `rgba(${c.r}, ${c.g}, ${c.b}, ${round(c.a, 2)})`
    : `rgb(${c.r}, ${c.g}, ${c.b})`;
}

export function toHsl(c: Rgba): string {
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;
  const d = max - min;
  if (d !== 0) {
    s = d / (1 - Math.abs(2 * l - 1));
    switch (max) {
      case r:
        h = ((g - b) / d) % 6;
        break;
      case g:
        h = (b - r) / d + 2;
        break;
      default:
        h = (r - g) / d + 4;
    }
    h *= 60;
    if (h < 0) h += 360;
  }
  const H = Math.round(h);
  const S = Math.round(s * 100);
  const L = Math.round(l * 100);
  return c.a < 1 ? `hsla(${H}, ${S}%, ${L}%, ${round(c.a, 2)})` : `hsl(${H}, ${S}%, ${L}%)`;
}

/// CSS-строка для показа образца (с учётом альфы).
export function toCss(c: Rgba): string {
  return `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})`;
}

function round(n: number, digits: number): number {
  const p = 10 ** digits;
  return Math.round(n * p) / p;
}
