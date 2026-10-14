// ═══════════════════════════════════════════════════════════
// Decor - zone plaques, sunlight dust and the archive hotspot
// ═══════════════════════════════════════════════════════════

import { FILING_CABINET, SUNBEAM, ZONE_SIGNS } from "./layout";
import { addRenderHook, addUpdateHook, isCabinetHovered, Layer } from "./canvas";
import { INK, pixelFont, pixelPanel } from "./pixel";

interface Mote {
  x: number;
  y: number;
  vx: number;
  vy: number;
  phase: number;
}

const motes: Mote[] = [];
let clock = 0;

function spawnMote(anywhere: boolean): Mote {
  return {
    x: SUNBEAM.x + Math.random() * SUNBEAM.w,
    y: anywhere ? SUNBEAM.y + Math.random() * SUNBEAM.h : SUNBEAM.y,
    vx: (Math.random() - 0.5) * 2.5,
    vy: 1 + Math.random() * 2.5,
    phase: Math.random() * Math.PI * 2,
  };
}

for (let i = 0; i < 26; i++) motes.push(spawnMote(true));

function update(dt: number): void {
  clock += dt;
  for (let i = 0; i < motes.length; i++) {
    const m = motes[i];
    m.x += (m.vx + Math.sin(clock * 0.7 + m.phase) * 1.5) * dt;
    m.y += m.vy * dt;
    if (m.y > SUNBEAM.y + SUNBEAM.h || m.x < SUNBEAM.x - 10 || m.x > SUNBEAM.x + SUNBEAM.w + 10) {
      motes[i] = spawnMote(false);
    }
  }
}

function renderDust(ctx: CanvasRenderingContext2D): void {
  for (const m of motes) {
    const fadeIn = Math.min(1, (m.y - SUNBEAM.y) / 12);
    const fadeOut = Math.min(1, (SUNBEAM.y + SUNBEAM.h - m.y) / 20);
    const twinkle = 0.45 + 0.4 * Math.sin(clock * 2 + m.phase);
    ctx.fillStyle = `rgba(255, 246, 214, ${Math.max(0, twinkle * fadeIn * fadeOut)})`;
    ctx.fillRect(Math.round(m.x), Math.round(m.y), 1, 1);
  }
}

function renderLabels(ctx: CanvasRenderingContext2D): void {
  ctx.font = pixelFont(6, 600);
  ctx.textAlign = "center";
  for (const sign of ZONE_SIGNS) {
    const w = Math.ceil(ctx.measureText(sign.label).width) + 8;
    const x = Math.round(sign.x - w / 2);
    const y = sign.y - 4;
    ctx.globalAlpha = 0.82;
    pixelPanel(ctx, x, y, w, 9, "#3a3044", INK);
    ctx.globalAlpha = 1;
    ctx.fillStyle = "#e9dcc0";
    ctx.fillText(sign.label, sign.x, y + 6.8);
  }

  if (isCabinetHovered()) {
    const { x, y, w, h } = FILING_CABINET;
    const pulse = 0.5 + 0.5 * Math.sin(clock * 6);
    ctx.fillStyle = `rgba(255, 236, 150, ${0.1 + 0.08 * pulse})`;
    ctx.fillRect(x, y, w, h);
    ctx.fillStyle = "#ffe68a";
    ctx.fillRect(x, y, w, 1);
    ctx.fillRect(x, y + h - 1, w, 1);
    ctx.fillRect(x, y, 1, h);
    ctx.fillRect(x + w - 1, y, 1, h);
    ctx.font = pixelFont(7, 600);
    const label = "RECORDS";
    const lw = Math.ceil(ctx.measureText(label).width) + 10;
    // Tag sits inside the hotspot so it never covers the archivist below.
    pixelPanel(ctx, x + w / 2 - lw / 2, y + h - 14, lw, 11, "#ffe68a");
    ctx.fillStyle = INK;
    ctx.fillText(label, x + w / 2, y + h - 5.5);
  }
  ctx.textAlign = "left";
}

export function initDecor(): void {
  addUpdateHook(update);
  addRenderHook(renderDust, Layer.World - 1);
  addRenderHook(renderLabels, Layer.Labels - 1);
}
