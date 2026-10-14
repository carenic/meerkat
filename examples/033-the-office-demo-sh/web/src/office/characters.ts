// ═══════════════════════════════════════════════════════════
// Characters - per-agent animation state + workstation rendering
// Each workstation is drawn as one depth-sorted unit:
// seated character → desk (hides the legs) → desk phone.
// ═══════════════════════════════════════════════════════════

import { AGENTS, AGENT_IDS } from "../types";
import type { AgentId, CharacterState } from "../types";
import { CHAR_BACK, DESKS } from "./layout";
import { Frame, getChar, getDesk, getProp, headPoint, phoneSlot } from "./sprites";
import { addRenderHook, addUpdateHook, getHoveredAgent, getSelectedAgent, Layer } from "./canvas";
import { INK, pixelFont, withAlpha } from "./pixel";

interface CharState {
  state: CharacterState;
  frame: Frame;
  /** Seconds left in the current typing burst (>0) or pause (<=0). */
  burst: number;
  pause: number;
  keyTimer: number;
  breath: number;
  talkTimer: number;
  talkUp: boolean;
  ring: number;
  /** Seconds since entering the current state (for small pop animations). */
  since: number;
}

const agents = new Map<AgentId, CharState>();

function rand(lo: number, hi: number): number {
  return lo + Math.random() * (hi - lo);
}

for (const id of AGENT_IDS) {
  agents.set(id, {
    state: "idle",
    frame: Frame.TypingA,
    burst: Math.random() < 0.5 ? rand(0.5, 2.5) : 0,
    pause: rand(0.2, 2.5),
    keyTimer: 0,
    breath: Math.random() * Math.PI * 2,
    talkTimer: 0,
    talkUp: false,
    ring: 0,
    since: 10,
  });
}

// ── Public API ──

export function setAgentState(id: AgentId, state: CharacterState): void {
  const a = agents.get(id);
  if (!a || a.state === state) return;
  a.state = state;
  a.since = 0;
  if (state === "on_call") a.ring = 0;
}

export function getAgentState(id: AgentId): CharacterState {
  return agents.get(id)?.state ?? "idle";
}

/** Make an agent's desk phone ring for a moment (incoming call). */
export function ringPhone(id: AgentId, seconds = 1.4): void {
  const a = agents.get(id);
  if (a && a.state !== "on_call") a.ring = Math.max(a.ring, seconds);
}

// ── Update ──

function update(dt: number): void {
  for (const [, a] of agents) {
    a.since += dt;
    a.breath += dt * 2.2;
    a.ring = Math.max(0, a.ring - dt);

    if (a.state === "on_call") {
      a.frame = Frame.Phone;
      a.talkTimer -= dt;
      if (a.talkTimer <= 0) {
        a.talkUp = !a.talkUp;
        a.talkTimer = rand(0.18, 0.42);
      }
    } else if (a.state === "thinking") {
      a.frame = Frame.Thinking;
    } else {
      updateTyping(a, dt);
    }
  }
}

/** Typing comes in bursts of keystrokes separated by short pauses. */
function updateTyping(a: CharState, dt: number): void {
  if (a.burst > 0) {
    a.burst -= dt;
    a.keyTimer -= dt;
    if (a.keyTimer <= 0) {
      a.frame = a.frame === Frame.TypingA ? Frame.TypingB : Frame.TypingA;
      a.keyTimer = rand(0.1, 0.19);
    }
    if (a.burst <= 0) {
      a.pause = rand(0.6, 2.8);
      a.frame = Frame.TypingA;
    }
  } else {
    a.frame = Frame.TypingA;
    a.pause -= dt;
    if (a.pause <= 0) a.burst = rand(0.8, 3.2);
  }
}

// ── Render ──

function renderWorld(ctx: CanvasRenderingContext2D): void {
  const sorted = [...AGENT_IDS].sort((a, b) => DESKS[a].y - DESKS[b].y);
  for (const id of sorted) drawWorkstation(ctx, id);
}

function drawWorkstation(ctx: CanvasRenderingContext2D, id: AgentId): void {
  const desk = DESKS[id];
  const a = agents.get(id)!;
  const sprite = getChar(id);
  const deskImg = getDesk(id);

  // Character: breathing bob when idle/thinking, nodding while talking.
  if (sprite) {
    let bob = Math.sin(a.breath) > 0.6 ? -1 : 0;
    if (a.state === "on_call") bob = a.talkUp ? -1 : 0;
    const x = Math.round(desk.x - sprite.frameW / 2);
    const y = Math.round(desk.y - CHAR_BACK - sprite.frameH + bob);
    ctx.drawImage(sprite.img, a.frame * sprite.frameW, 0, sprite.frameW, sprite.frameH, x, y, sprite.frameW, sprite.frameH);
  }

  if (deskImg) {
    ctx.drawImage(deskImg, Math.round(desk.x - deskImg.width / 2), desk.y - deskImg.height);
  }

  drawPhone(ctx, id, a);
}

function drawPhone(ctx: CanvasRenderingContext2D, id: AgentId, a: CharState): void {
  const offHook = a.state === "on_call";
  const img = getProp(offHook ? "phone_off" : "phone");
  if (!img) return;
  const slot = phoneSlot(id);
  let dx = 0;
  let dy = 0;
  if (a.ring > 0) {
    // Ringing: a jittery shake plus two pairs of ring marks.
    const t = performance.now() / 1000;
    dx = Math.round(Math.sin(t * 60) * 1);
    dy = Math.sin(t * 45) > 0.3 ? -1 : 0;
    if (Math.sin(t * 9) > -0.2) drawRingMarks(ctx, slot.x + img.width / 2, slot.y - img.height - 1);
  }
  ctx.drawImage(img, slot.x + dx, slot.y - img.height + dy);
}

function drawRingMarks(ctx: CanvasRenderingContext2D, cx: number, cy: number): void {
  ctx.fillStyle = "#fff7d6";
  for (const side of [-1, 1]) {
    const x = Math.round(cx + side * 9);
    ctx.fillRect(x, cy - 4, 1, 3);
    ctx.fillRect(x + side, cy - 6, 1, 2);
    ctx.fillRect(x + side * 3, cy - 3, 1, 3);
    ctx.fillRect(x + side * 4, cy - 6, 1, 2);
  }
}

function renderLabels(ctx: CanvasRenderingContext2D): void {
  const selected = getSelectedAgent();
  const hovered = getHoveredAgent();
  for (const id of AGENT_IDS) {
    drawNameplate(ctx, id, id === selected, id === hovered);
    if (id === selected) drawSelectionMarker(ctx, id);
  }
}

/** A small brass-and-ink plate on the desk's front panel. */
function drawNameplate(ctx: CanvasRenderingContext2D, id: AgentId, selected: boolean, hovered: boolean): void {
  const desk = DESKS[id];
  const meta = AGENTS[id];
  const text = desk.label;
  ctx.font = pixelFont(7, 600);
  const textW = Math.ceil(ctx.measureText(text).width);
  const w = textW + 10;
  const h = 10;
  const x = Math.round(desk.x - w / 2);
  const y = desk.y - 13;
  const lit = selected || hovered;

  ctx.fillStyle = lit ? meta.color : INK;
  ctx.fillRect(x + 1, y, w - 2, h);
  ctx.fillRect(x, y + 1, w, h - 2);
  ctx.fillStyle = lit ? "#2b2233" : "#3a3044";
  ctx.fillRect(x + 1, y + 1, w - 2, h - 2);
  // Agent colour chip.
  ctx.fillStyle = meta.color;
  ctx.fillRect(x + 2, y + 2, 2, h - 4);

  ctx.textAlign = "center";
  ctx.fillStyle = lit ? "#ffffff" : "#f3e9d2";
  ctx.fillText(text, x + w / 2 + 1.5, y + 7.6);

  if (selected || hovered) {
    ctx.font = pixelFont(5.5, 500);
    const role = meta.role;
    const rw = Math.ceil(ctx.measureText(role).width) + 8;
    const rx = Math.round(desk.x - rw / 2);
    const ry = y + h + 1;
    ctx.fillStyle = withAlpha("#2b2233", 0.88);
    ctx.fillRect(rx, ry, rw, 8);
    ctx.fillStyle = "#d9cfe6";
    ctx.fillText(role, desk.x, ry + 6);
  }
  ctx.textAlign = "left";
}

/** Bouncing pixel arrow over the selected agent's head. */
function drawSelectionMarker(ctx: CanvasRenderingContext2D, id: AgentId): void {
  const head = headPoint(id);
  const color = AGENTS[id].color;
  const bounce = Math.round(Math.abs(Math.sin(performance.now() / 260)) * 3);
  const x = Math.round(head.x);
  const y = Math.round(head.y - 11 - bounce);
  const rows = [11, 9, 7, 5, 3, 1];
  ctx.fillStyle = INK;
  ctx.fillRect(x - 6, y - 1, 13, 1);
  rows.forEach((w, i) => ctx.fillRect(x - (w >> 1) - 1, y + i, w + 2, 1));
  ctx.fillRect(x, y + rows.length, 1, 1);
  ctx.fillStyle = color;
  rows.forEach((w, i) => ctx.fillRect(x - (w >> 1), y + i, w, 1));
}

export function initCharacters(): void {
  addUpdateHook(update);
  addRenderHook(renderWorld, Layer.World);
  addRenderHook(renderLabels, Layer.Labels);
}
