// ═══════════════════════════════════════════════════════════
// Pixel helpers - shared drawing primitives in art space
// ═══════════════════════════════════════════════════════════

/** Readable pixel face for in-world text; sizes are in art pixels. */
export const PIXEL_FONT_FAMILY = "'Pixelify Sans', 'Chakra Petch', sans-serif";
export const pixelFont = (size: number, weight = 600): string => `${weight} ${size}px ${PIXEL_FONT_FAMILY}`;

export const INK = "#2b2233";
export const PAPER = "#fffaf0";

/**
 * A panel with notched (pixel-rounded) corners and a 1px outline.
 * Coordinates are snapped to whole art pixels.
 */
export function pixelPanel(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  fill: string,
  outline = INK,
): void {
  x = Math.round(x);
  y = Math.round(y);
  w = Math.round(w);
  h = Math.round(h);
  ctx.fillStyle = outline;
  ctx.fillRect(x + 1, y, w - 2, h);
  ctx.fillRect(x, y + 1, w, h - 2);
  ctx.fillStyle = fill;
  ctx.fillRect(x + 1, y + 1, w - 2, h - 2);
}

/** Drop shadow one pixel down/right of a panel, for a little lift. */
export function pixelShadow(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number): void {
  ctx.fillStyle = "rgba(20, 12, 28, 0.28)";
  ctx.fillRect(Math.round(x) + 2, Math.round(y) + h, Math.round(w) - 2, 1);
  ctx.fillRect(Math.round(x) + w, Math.round(y) + 2, 1, Math.round(h) - 2);
}

/** Text with a 1px dark outline, readable over any part of the room. */
export function outlinedText(
  ctx: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  fill: string,
  outline = INK,
  width = 0.6,
): void {
  ctx.lineJoin = "round";
  ctx.lineWidth = width * 2;
  ctx.strokeStyle = outline;
  ctx.strokeText(text, x, y);
  ctx.fillStyle = fill;
  ctx.fillText(text, x, y);
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

export function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}

export function easeOutBack(t: number): number {
  const c1 = 1.70158;
  const c3 = c1 + 1;
  return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2);
}

/** Hex colour to an rgba() string. */
export function withAlpha(hex: string, alpha: number): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** Mix a hex colour toward black (amount < 0) or white (amount > 0). */
export function shade(hex: string, amount: number): string {
  const n = parseInt(hex.slice(1), 16);
  const target = amount < 0 ? 0 : 255;
  const t = Math.abs(amount);
  const mix = (c: number) => Math.round(c + (target - c) * t);
  return `rgb(${mix((n >> 16) & 255)}, ${mix((n >> 8) & 255)}, ${mix(n & 255)})`;
}
