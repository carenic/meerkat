// ═══════════════════════════════════════════════════════════
// Bubbles - pixel speech bubbles + thought clouds
// Speech bubbles show their full text immediately and choose a spot
// that avoids other agents and bubbles. All in art space.
// ═══════════════════════════════════════════════════════════

import { AGENTS, AGENT_IDS } from "../types";
import type { AgentId } from "../types";
import { ART_H, ART_W, DESKS } from "./layout";
import { agentHitRect, headPoint } from "./sprites";
import { addRenderHook, addUpdateHook, Layer } from "./canvas";
import { INK, PAPER, clamp, easeOutBack, pixelFont, pixelPanel, pixelShadow } from "./pixel";

interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface ActiveBubble {
  agentId: AgentId;
  text: string;
  duration: number;
  elapsed: number;
}

interface BubbleLayout extends Rect {
  lines: string[];
  speakerX: number;
  speakerY: number;
}

const speechBubbles = new Map<AgentId, ActiveBubble>();
const thinkingAgents = new Set<AgentId>();
let clock = 0;

const FONT = pixelFont(7, 500);
const LINE_H = 8;
const PAD_X = 5;
const PAD_Y = 3;
const MAX_TEXT_W = 116;
const MAX_LINES = 3;
const MARGIN = 3;
/** Most speech bubbles shown at once; more than this buries the room. */
const MAX_VISIBLE = 4;
/** A quick scale pop; the full text is visible from the first frame. */
const POP_TIME = 0.08;
const FADE_TIME = 0.35;

// ── Public API ──

export function showSpeechBubble(agentId: AgentId, text: string, durationMs = 4000): void {
  const clean = text.replace(/\s+/g, " ").trim();
  const truncated = clean.length > 96 ? clean.slice(0, 93) + "..." : clean;
  // At most MAX_VISIBLE bubbles at once: a new speaker retires the oldest.
  if (!speechBubbles.has(agentId)) {
    const live = [...speechBubbles.values()].filter((b) => b.duration - b.elapsed > FADE_TIME);
    if (live.length >= MAX_VISIBLE) {
      const oldest = live.reduce((a, b) => (b.elapsed > a.elapsed ? b : a));
      oldest.elapsed = oldest.duration - FADE_TIME;
    }
  }
  speechBubbles.set(agentId, { agentId, text: truncated, duration: durationMs / 1000, elapsed: 0 });
}

export function hideSpeechBubble(agentId: AgentId): void {
  speechBubbles.delete(agentId);
}

export function showThinkBubble(agentId: AgentId): void {
  thinkingAgents.add(agentId);
}

export function hideThinkBubble(agentId: AgentId): void {
  thinkingAgents.delete(agentId);
}

export function isThinking(agentId: AgentId): boolean {
  return thinkingAgents.has(agentId);
}

// ── Update ──

function update(dt: number): void {
  clock += dt;
  for (const [id, b] of speechBubbles) {
    b.elapsed += dt;
    if (b.elapsed >= b.duration) speechBubbles.delete(id);
  }
}

// ── Render ──

function render(ctx: CanvasRenderingContext2D): void {
  const placed: Rect[] = [];
  const ordered = [...speechBubbles.values()].sort((a, b) => DESKS[a.agentId].y - DESKS[b.agentId].y);
  for (const bubble of ordered) {
    const layout = layoutSpeech(ctx, bubble, placed);
    drawSpeech(ctx, bubble, layout);
    placed.push(layout);
  }
  for (const id of thinkingAgents) {
    if (!speechBubbles.has(id)) drawThink(ctx, id, placed);
  }
}

function layoutSpeech(ctx: CanvasRenderingContext2D, bubble: ActiveBubble, placed: Rect[]): BubbleLayout {
  const head = headPoint(bubble.agentId);
  const sx = head.x;
  const sy = head.y - 1;
  ctx.font = FONT;
  const lines = wrapText(ctx, bubble.text, MAX_TEXT_W, MAX_LINES);
  const textW = Math.max(...lines.map((line) => ctx.measureText(line).width));
  const w = Math.ceil(textW) + PAD_X * 2;
  const h = lines.length * LINE_H + PAD_Y * 2 + 1;

  // Faces (everyone's, the speaker's included) and other bubbles are
  // near-hard obstacles; desks and nameplates are soft ones.
  const desks = AGENT_IDS.filter((id) => id !== bubble.agentId).map(agentHitRect);
  const faces = AGENT_IDS.map((id) => {
    const r = agentHitRect(id);
    return { x: r.x + 12, y: r.y - 2, w: r.w - 24, h: 30 };
  });
  const options = candidates(sx, sy, w, h);
  let best = clampRect(options[0]);
  let bestScore = Infinity;
  for (let i = 0; i < options.length; i++) {
    const c = options[i];
    const r = clampRect(c);
    const displacement = Math.abs(r.x - c.x) + Math.abs(r.y - c.y);
    let overlap = 0;
    for (const o of desks) overlap += intersection(r, o) * 0.5;
    for (const f of faces) overlap += intersection(r, f) * 12;
    for (const p of placed) overlap += intersection(r, p) * 12;
    const score = overlap + displacement * 4 + i * 30;
    if (score < bestScore) {
      bestScore = score;
      best = r;
    }
  }
  return { ...best, lines, speakerX: sx, speakerY: sy };
}

function candidates(sx: number, sy: number, w: number, h: number): Rect[] {
  const above = sy - h - 5;
  return [
    { x: sx - w / 2, y: above, w, h },
    { x: sx - w + 14, y: above, w, h },
    { x: sx - 14, y: above, w, h },
    { x: sx - w / 2, y: above - 22, w, h },
    { x: sx + 20, y: sy + 4, w, h },
    { x: sx - w - 20, y: sy + 4, w, h },
  ].map((r) => ({ x: Math.round(r.x), y: Math.round(r.y), w, h }));
}

function drawSpeech(ctx: CanvasRenderingContext2D, bubble: ActiveBubble, l: BubbleLayout): void {
  const remaining = bubble.duration - bubble.elapsed;
  const alpha = remaining < FADE_TIME ? Math.max(0, remaining / FADE_TIME) : 1;
  const pop = 0.85 + 0.15 * easeOutBack(Math.min(1, bubble.elapsed / POP_TIME));
  const tail = tailAnchor(l);

  ctx.save();
  ctx.globalAlpha = alpha;
  // Pop in around the tail so the bubble seems to come out of the speaker.
  ctx.translate(tail.x, tail.y);
  ctx.scale(pop, pop);
  ctx.translate(-tail.x, -tail.y);

  drawLeader(ctx, bubble.agentId, l, tail);
  pixelShadow(ctx, l.x, l.y, l.w, l.h);
  drawTail(ctx, l, tail);
  pixelPanel(ctx, l.x, l.y, l.w, l.h, PAPER);
  // Accent stripe in the speaker's colour.
  ctx.fillStyle = AGENTS[bubble.agentId].color;
  ctx.fillRect(l.x + 2, l.y + l.h - 2, l.w - 4, 1);

  ctx.font = FONT;
  ctx.fillStyle = INK;
  ctx.textAlign = "left";
  for (let i = 0; i < l.lines.length; i++) {
    ctx.fillText(l.lines[i], l.x + PAD_X, l.y + PAD_Y + (i + 1) * LINE_H - 1.5);
  }
  ctx.restore();
}

/** When a bubble had to move away from its speaker, a dotted leader keeps ownership clear. */
function drawLeader(ctx: CanvasRenderingContext2D, id: AgentId, l: BubbleLayout, tail: { x: number; y: number }): void {
  const dx = l.speakerX - tail.x;
  const dy = l.speakerY - tail.y;
  const dist = Math.hypot(dx, dy);
  if (dist < 10) return;
  ctx.fillStyle = AGENTS[id].color;
  for (let d = 4; d < dist - 2; d += 3) {
    ctx.fillRect(Math.round(tail.x + (dx * d) / dist), Math.round(tail.y + (dy * d) / dist), 1, 1);
  }
}

type TailSide = "bottom" | "top" | "left" | "right";

function tailAnchor(l: BubbleLayout): { x: number; y: number; side: TailSide } {
  if (l.y + l.h <= l.speakerY) {
    return { x: clamp(Math.round(l.speakerX), l.x + 5, l.x + l.w - 6), y: l.y + l.h, side: "bottom" };
  }
  if (l.y >= l.speakerY) {
    if (l.x >= l.speakerX) return { x: l.x, y: clamp(l.speakerY, l.y + 4, l.y + l.h - 5), side: "left" };
    if (l.x + l.w <= l.speakerX) return { x: l.x + l.w, y: clamp(l.speakerY, l.y + 4, l.y + l.h - 5), side: "right" };
    return { x: clamp(Math.round(l.speakerX), l.x + 5, l.x + l.w - 6), y: l.y, side: "top" };
  }
  return l.x >= l.speakerX
    ? { x: l.x, y: l.y + 5, side: "left" }
    : { x: l.x + l.w, y: l.y + 5, side: "right" };
}

/**
 * A stepped pixel wedge leaning toward the speaker. Row 0 sits on the
 * panel outline and opens it; each later row narrows to a 1px tip.
 */
function drawTail(ctx: CanvasRenderingContext2D, l: BubbleLayout, t: { x: number; y: number; side: TailSide }): void {
  const vertical = t.side === "bottom" || t.side === "top";
  const dir = t.side === "bottom" || t.side === "right" ? 1 : -1;
  const lean = vertical ? Math.sign(l.speakerX - t.x) : Math.sign(l.speakerY - t.y);
  // Row 0 overlaps the panel's outline row/column.
  const origin = t.side === "bottom" || t.side === "right" ? -1 : 0;
  for (let i = 0; i < 4; i++) {
    const half = 3 - i;
    const along = (vertical ? t.y : t.x) + origin + dir * i;
    const across = (vertical ? t.x : t.y) + lean * Math.floor(i / 2);
    const fill = (from: number, len: number, color: string) => {
      ctx.fillStyle = color;
      if (vertical) ctx.fillRect(from, along, len, 1);
      else ctx.fillRect(along, from, 1, len);
    };
    fill(across - half, half * 2 + 1, INK);
    if (half >= 1) fill(across - half + 1, half * 2 - 1, PAPER);
  }
}

/** A small pixel cloud with three cycling dots, trailing puffs toward the head. */
function drawThink(ctx: CanvasRenderingContext2D, id: AgentId, placed: Rect[]): void {
  const head = headPoint(id);
  const w = 21;
  const h = 10;
  let x = Math.round(head.x + 4);
  let y = Math.round(head.y - h - 9);
  for (let i = 0; i < 4 && placed.some((p) => intersection({ x, y: y - 3, w, h: h + 3 }, p) > 0); i++) y -= 10;
  x = clamp(x, MARGIN, ART_W - w - MARGIN);
  y = clamp(y, MARGIN + 3, ART_H - h - MARGIN);

  const bob = Math.sin(clock * 2.4) > 0 ? 0 : -1;
  y += bob;

  // Trailing puffs.
  pixelPanel(ctx, head.x + 1, head.y - 5 + bob, 3, 3, PAPER);
  pixelPanel(ctx, head.x + 4, head.y - 9 + bob, 4, 4, PAPER);
  // Cloud: body plus two bumps on top.
  pixelShadow(ctx, x, y, w, h);
  pixelPanel(ctx, x + 3, y - 3, 7, 5, PAPER);
  pixelPanel(ctx, x + 10, y - 2, 8, 4, PAPER);
  pixelPanel(ctx, x, y, w, h, PAPER);
  ctx.fillStyle = PAPER;
  ctx.fillRect(x + 4, y, 5, 1);
  ctx.fillRect(x + 11, y, 6, 1);

  const active = Math.floor(clock * 3) % 3;
  for (let i = 0; i < 3; i++) {
    ctx.fillStyle = i === active ? INK : "#b7adc2";
    ctx.fillRect(x + 5 + i * 4, y + 4, 2, 2);
  }
}

// ── Geometry / text helpers ──

function clampRect(r: Rect): Rect {
  return {
    x: clamp(r.x, MARGIN, ART_W - MARGIN - r.w),
    y: clamp(r.y, MARGIN, ART_H - MARGIN - r.h),
    w: r.w,
    h: r.h,
  };
}

function intersection(a: Rect, b: Rect): number {
  const w = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

function wrapText(ctx: CanvasRenderingContext2D, text: string, maxWidth: number, maxLines: number): string[] {
  const lines: string[] = [];
  let current = "";
  for (const word of text.split(" ")) {
    const next = current ? `${current} ${word}` : word;
    if (ctx.measureText(next).width <= maxWidth || !current) {
      current = next;
    } else {
      lines.push(current);
      current = word;
    }
  }
  if (current) lines.push(current);

  // Hard-break any single word that is still too wide.
  const broken = lines.flatMap((line) => {
    if (ctx.measureText(line).width <= maxWidth) return [line];
    const parts: string[] = [];
    let part = "";
    for (const ch of line) {
      if (ctx.measureText(part + ch).width > maxWidth && part) {
        parts.push(part);
        part = ch;
      } else {
        part += ch;
      }
    }
    if (part) parts.push(part);
    return parts;
  });

  if (broken.length > maxLines) {
    broken.length = maxLines;
    let tail = broken[maxLines - 1];
    while (tail.length > 1 && ctx.measureText(`${tail}...`).width > maxWidth) tail = tail.slice(0, -1);
    broken[maxLines - 1] = `${tail.trimEnd()}...`;
  }
  return broken;
}

export function initBubbles(): void {
  addUpdateHook(update);
  addRenderHook(render, Layer.Bubbles);
}
