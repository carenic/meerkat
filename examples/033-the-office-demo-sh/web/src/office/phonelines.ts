// ═══════════════════════════════════════════════════════════
// Phone Lines - call lines between desk phones, flying mail
// Each send flies an envelope from the caller's phone to the callee's,
// drawing the line behind it; the callee's phone rings on arrival. The
// line then settles into a slim trace that stays at least MIN_VISIBLE
// seconds, even when the caller's turn ends first. Mail and records fly
// with a floor shadow.
// ═══════════════════════════════════════════════════════════

import type { AgentId } from "../types";
import { CABINET_DRAWER, MAIL_SLOT } from "./layout";
import { getProp, inTrayPoint, phonePoint } from "./sprites";
import type { PropName } from "./sprites";
import { addRenderHook, addUpdateHook, Layer } from "./canvas";
import { ringPhone } from "./characters";
import { easeOutCubic, shade } from "./pixel";

interface ActiveCall {
  id: string;
  from: AgentId;
  to: AgentId;
  color: string;
  elapsed: number;
  /** Elapsed time of the latest send on this line; its envelope flies from here. */
  sentAt: number;
  /** The latest envelope has reached the callee. */
  arrived: boolean;
  /** The line is held at full strength until at least this elapsed time. */
  holdUntil: number;
  /** The caller's turn has ended; fade once the hold expires. */
  ended: boolean;
  fadeStart: number | null;
  opacity: number;
}

interface Flight {
  prop: PropName;
  from: { x: number; y: number };
  to: { x: number; y: number };
  /** Floor level under the start and end, for the shadow. */
  floorFrom: number;
  floorTo: number;
  arc: number;
  elapsed: number;
  duration: number;
  landed: number;
}

const calls = new Map<string, ActiveCall>();
const flights: Flight[] = [];
const flashes: { x: number; y: number; w: number; h: number; elapsed: number }[] = [];
let nextCallId = 0;

const MIN_VISIBLE = 6;
const FADE_DURATION = 1.5;
/** Seconds for the envelope to fly the length of the line. */
const TRAVEL_TIME = 0.9;
/** Seconds after arrival that the line keeps its fresh glow. */
const FRESH_TIME = 0.8;
const SPARKLE_TIME = 0.45;

// ── Public API ──

export function startCall(from: AgentId, to: AgentId, color = "#4488CC"): string {
  // A repeat send on the same line flies a new envelope along it instead of
  // stacking a copy.
  for (const [, c] of calls) {
    if (c.from === from && c.to === to) {
      c.color = color;
      c.sentAt = c.elapsed;
      c.arrived = false;
      c.holdUntil = c.elapsed + TRAVEL_TIME + MIN_VISIBLE;
      c.ended = false;
      c.fadeStart = null;
      return c.id;
    }
  }
  const id = `call-${nextCallId++}`;
  calls.set(id, {
    id, from, to, color, elapsed: 0, sentAt: 0, arrived: false,
    holdUntil: TRAVEL_TIME + MIN_VISIBLE, ended: false, fadeStart: null, opacity: 0,
  });
  return id;
}

export function endCall(callId: string): void {
  const c = calls.get(callId);
  if (c) c.ended = true;
}

export function endCallsForAgent(agentId: AgentId): void {
  for (const [, c] of calls) {
    if (c.from === agentId || c.to === agentId) c.ended = true;
  }
}

export function getActiveCallCount(): number { return calls.size; }

/** An envelope pops out of the wall mail slot and lands in the agent's in-tray. */
export function triggerEnvelopeArrival(toAgent: AgentId): void {
  const tray = inTrayPoint(toAgent);
  flashes.push({ x: MAIL_SLOT.x - 8, y: MAIL_SLOT.y - 3, w: 16, h: 6, elapsed: 0 });
  flights.push({
    prop: "envelope",
    from: { x: MAIL_SLOT.x, y: MAIL_SLOT.y },
    to: tray,
    floorFrom: 72,
    floorTo: tray.y + 30,
    arc: 34,
    elapsed: 0,
    duration: 1.25,
    landed: 0,
  });
}

/** A folder flies from an agent's desk into the archive's filing cabinet. */
export function triggerFileRecord(fromAgent: AgentId = "archivist"): void {
  const start = inTrayPoint(fromAgent);
  flights.push({
    prop: "folder",
    from: start,
    to: CABINET_DRAWER,
    floorFrom: start.y + 30,
    floorTo: 66,
    arc: 30,
    elapsed: 0,
    duration: 1.05,
    landed: 0,
  });
}

// ── Update ──

function update(dt: number): void {
  for (const [id, c] of calls) {
    c.elapsed += dt;
    if (!c.arrived && c.elapsed - c.sentAt >= TRAVEL_TIME) {
      c.arrived = true;
      ringPhone(c.to);
    }
    if (c.fadeStart === null && c.ended && c.elapsed >= c.holdUntil) c.fadeStart = c.elapsed;
    if (c.fadeStart !== null) {
      c.opacity = Math.max(0, 1 - (c.elapsed - c.fadeStart) / FADE_DURATION);
      if (c.opacity <= 0) calls.delete(id);
    } else {
      c.opacity = Math.min(1, c.elapsed / 0.12);
    }
  }

  for (let i = flights.length - 1; i >= 0; i--) {
    const f = flights[i];
    if (f.elapsed < f.duration) {
      f.elapsed = Math.min(f.duration, f.elapsed + dt);
      if (f.elapsed >= f.duration && f.prop === "folder") {
        flashes.push({ x: CABINET_DRAWER.x - 14, y: CABINET_DRAWER.y - 3, w: 28, h: 6, elapsed: 0 });
      }
    } else {
      f.landed += dt;
      if (f.landed >= SPARKLE_TIME) flights.splice(i, 1);
    }
  }

  for (let i = flashes.length - 1; i >= 0; i--) {
    flashes[i].elapsed += dt;
    if (flashes[i].elapsed > 0.35) flashes.splice(i, 1);
  }
}

// ── Render ──

function render(ctx: CanvasRenderingContext2D): void {
  for (const [, c] of calls) renderCall(ctx, c);
  for (const f of flashes) {
    ctx.fillStyle = `rgba(255, 244, 196, ${0.7 * (1 - f.elapsed / 0.35)})`;
    ctx.fillRect(f.x, f.y, f.w, f.h);
  }
  for (const f of flights) renderFlight(ctx, f);
}

function curvePoint(
  x1: number, y1: number, cx: number, cy: number, x2: number, y2: number, t: number,
): { x: number; y: number } {
  const u = 1 - t;
  return { x: u * u * x1 + 2 * u * t * cx + t * t * x2, y: u * u * y1 + 2 * u * t * cy + t * t * y2 };
}

function renderCall(ctx: CanvasRenderingContext2D, c: ActiveCall): void {
  const a = phonePoint(c.from);
  const b = phonePoint(c.to);
  const cx = (a.x + b.x) / 2;
  const cy = Math.min(a.y, b.y) - 22 - Math.abs(b.x - a.x) * 0.14;
  const length = Math.hypot(b.x - a.x, b.y - a.y) + Math.abs(cy - Math.min(a.y, b.y));
  const steps = Math.max(16, Math.ceil(length));
  const points: { x: number; y: number }[] = [];
  for (let k = 0; k <= steps; k++) {
    const p = curvePoint(a.x, a.y, cx, cy, b.x, b.y, k / steps);
    points.push({ x: Math.round(p.x), y: Math.round(p.y) });
  }

  ctx.save();

  // How far the latest envelope has flown, and how fresh the line still is.
  const flight = Math.min(1, (c.elapsed - c.sentAt) / TRAVEL_TIME);
  const head = Math.round(flight * steps);
  const sinceArrival = c.elapsed - c.sentAt - TRAVEL_TIME;
  const fresh = sinceArrival < FRESH_TIME ? 1 - Math.max(0, sinceArrival) / FRESH_TIME : 0;
  // A first send draws the line behind its envelope; a repeat send flies its
  // envelope along the line that already exists.
  const drawn = c.arrived || c.sentAt > 0 ? points : points.slice(0, head + 1);

  // Soft glow while the send is fresh, so a new message catches the eye.
  if (fresh > 0) {
    ctx.beginPath();
    ctx.moveTo(drawn[0].x, drawn[0].y);
    for (const p of drawn) ctx.lineTo(p.x, p.y);
    ctx.strokeStyle = c.color;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.lineWidth = 6;
    ctx.globalAlpha = c.opacity * 0.25 * fresh;
    ctx.stroke();
  }

  // Slim pixel line: dark outline with a colour core; a fresh line is bolder.
  ctx.globalAlpha = c.opacity;
  const bold = fresh > 0.35;
  ctx.fillStyle = shade(c.color, -0.55);
  for (const p of drawn) ctx.fillRect(p.x - (bold ? 2 : 1), p.y - (bold ? 2 : 1), bold ? 4 : 3, bold ? 4 : 3);
  ctx.fillStyle = c.color;
  for (const p of drawn) ctx.fillRect(p.x - (bold ? 1 : 0), p.y - (bold ? 1 : 0), bold ? 2 : 1, bold ? 2 : 1);

  // The envelope rides the head of the line until it reaches the callee.
  if (flight < 1) {
    const p = points[Math.min(points.length - 1, head)];
    const img = getProp("envelope");
    if (img) ctx.drawImage(img, Math.round(p.x - img.width / 2), Math.round(p.y - img.height / 2) - 1);
  } else if (sinceArrival < 0.4) {
    // Arrival burst at the callee's phone.
    const s = sinceArrival / 0.4;
    const d = Math.round(3 + s * 6);
    ctx.globalAlpha = c.opacity * (1 - s);
    ctx.fillStyle = shade(c.color, 0.6);
    for (const [dx, dy] of [[0, -1], [1, 0], [0, 1], [-1, 0], [1, -1], [-1, -1]]) {
      ctx.fillRect(Math.round(b.x + dx * d), Math.round(b.y + dy * d), 2, 2);
    }
  }

  ctx.restore();
}

function renderFlight(ctx: CanvasRenderingContext2D, f: Flight): void {
  const img = getProp(f.prop);
  const t = easeOutCubic(f.elapsed / f.duration);
  const x = f.from.x + (f.to.x - f.from.x) * t;
  const ground = f.from.y + (f.to.y - f.from.y) * t;
  const lift = Math.sin(t * Math.PI) * f.arc;
  const y = ground - lift;

  if (f.elapsed < f.duration) {
    // Floor shadow shrinks as the item rises.
    const floor = f.floorFrom + (f.floorTo - f.floorFrom) * t;
    const sw = Math.round(10 - (lift / Math.max(1, f.arc)) * 4);
    ctx.fillStyle = "rgba(30, 20, 40, 0.25)";
    ctx.fillRect(Math.round(x - sw / 2), Math.round(floor), sw, 2);
    ctx.fillRect(Math.round(x - sw / 2) + 1, Math.round(floor) - 1, sw - 2, 1);

    // Flutter: a 1px hop every few frames while airborne.
    const flutter = Math.sin(f.elapsed * 30) > 0.5 ? -1 : 0;
    if (img) ctx.drawImage(img, Math.round(x - img.width / 2), Math.round(y - img.height / 2) + flutter);
    return;
  }

  // Landed: the item settles and a few sparkles burst out.
  const s = f.landed / SPARKLE_TIME;
  if (img && f.prop === "envelope") {
    ctx.globalAlpha = 1 - s;
    ctx.drawImage(img, Math.round(f.to.x - img.width / 2), Math.round(f.to.y - img.height / 2));
    ctx.globalAlpha = 1;
  }
  const d = Math.round(4 + s * 6);
  ctx.fillStyle = `rgba(255, 236, 150, ${1 - s})`;
  for (const [dx, dy] of [[0, -1], [1, 0], [0, 1], [-1, 0]]) {
    ctx.fillRect(Math.round(f.to.x + dx * d), Math.round(f.to.y + dy * d), 2, 2);
  }
}

export function initPhoneLines(): void {
  addUpdateHook(update);
  addRenderHook(render, Layer.Effects);
}
