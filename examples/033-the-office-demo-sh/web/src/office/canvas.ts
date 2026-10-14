// ═══════════════════════════════════════════════════════════
// Office Canvas - game loop, scaling and input
// All drawing happens in 640x360 art space; the backing store
// matches the displayed size x devicePixelRatio, so pixel art is
// scaled with nearest-neighbour while text stays crisp.
// ═══════════════════════════════════════════════════════════

import { AGENT_IDS } from "../types";
import type { AgentId } from "../types";
import { ART_W, ART_H, DESKS, FILING_CABINET } from "./layout";
import { agentHitRect, getBackground } from "./sprites";

// ── State ──

let canvas: HTMLCanvasElement | null = null;
let ctx: CanvasRenderingContext2D | null = null;
let scale = 1;
let lastTime = 0;
let stopped = false;
let selectedAgent: AgentId | null = null;
let hoveredAgent: AgentId | null = null;
let cabinetHovered = false;
let onSelectAgent: ((id: AgentId | null) => void) | null = null;
let onFilingCabinetClick: (() => void) | null = null;

/** Draw order: lower layers first. */
export const Layer = {
  World: 10,
  Effects: 20,
  Labels: 30,
  Bubbles: 40,
} as const;

export type RenderHook = (ctx: CanvasRenderingContext2D, dt: number) => void;
const renderHooks: { layer: number; hook: RenderHook }[] = [];
export function addRenderHook(hook: RenderHook, layer: number = Layer.World): void {
  renderHooks.push({ layer, hook });
  renderHooks.sort((a, b) => a.layer - b.layer);
}

export type UpdateHook = (dt: number) => void;
const updateHooks: UpdateHook[] = [];
export function addUpdateHook(hook: UpdateHook): void { updateHooks.push(hook); }

// ── Init ──

export function initCanvas(el: HTMLCanvasElement): void {
  canvas = el;
  ctx = el.getContext("2d");
  const container = el.parentElement;
  if (container) new ResizeObserver(resizeCanvas).observe(container);
  window.addEventListener("resize", resizeCanvas);
  el.addEventListener("click", handleClick);
  el.addEventListener("mousemove", handleMove);
  el.addEventListener("mouseleave", () => setHover(null, false));
  resizeCanvas();
  lastTime = 0;
  stopped = false;
  requestAnimationFrame(frame);
}

export function setOnSelectAgent(cb: (id: AgentId | null) => void): void { onSelectAgent = cb; }
export function setOnFilingCabinetClick(cb: () => void): void { onFilingCabinetClick = cb; }
export function stopCanvas(): void { stopped = true; }

export function getSelectedAgent(): AgentId | null { return selectedAgent; }
/** Highlight an agent without notifying the selection callback (e.g. from the chat picker). */
export function highlightAgent(id: AgentId | null): void { selectedAgent = id; }
export function getHoveredAgent(): AgentId | null { return hoveredAgent; }
export function isCabinetHovered(): boolean { return cabinetHovered; }

// ── Resize (fit 16:9, backing store at device resolution) ──

function resizeCanvas(): void {
  if (!canvas) return;
  const container = canvas.parentElement;
  if (!container) return;
  const fit = Math.min(container.clientWidth / ART_W, container.clientHeight / ART_H);
  const cssW = Math.max(1, Math.floor(ART_W * fit));
  const cssH = Math.max(1, Math.floor(ART_H * fit));
  const dpr = window.devicePixelRatio || 1;
  canvas.style.width = `${cssW}px`;
  canvas.style.height = `${cssH}px`;
  canvas.width = Math.round(cssW * dpr);
  canvas.height = Math.round(cssH * dpr);
  scale = canvas.width / ART_W;
}

// ── Input ──

function toArt(e: MouseEvent): { x: number; y: number } | null {
  if (!canvas) return null;
  const rect = canvas.getBoundingClientRect();
  return {
    x: (e.clientX - rect.left) * (ART_W / rect.width),
    y: (e.clientY - rect.top) * (ART_H / rect.height),
  };
}

function inRect(p: { x: number; y: number }, r: { x: number; y: number; w: number; h: number }): boolean {
  return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

function agentAt(p: { x: number; y: number }): AgentId | null {
  // Front-most (largest y) desks win when hit boxes overlap.
  const order = [...AGENT_IDS].sort((a, b) => DESKS[b].y - DESKS[a].y);
  return order.find((id) => inRect(p, agentHitRect(id))) ?? null;
}

function setHover(agent: AgentId | null, cabinet: boolean): void {
  hoveredAgent = agent;
  cabinetHovered = cabinet;
  if (canvas) canvas.style.cursor = agent || cabinet ? "pointer" : "default";
}

function handleMove(e: MouseEvent): void {
  const p = toArt(e);
  if (!p) return;
  const agent = agentAt(p);
  setHover(agent, !agent && inRect(p, FILING_CABINET));
}

function handleClick(e: MouseEvent): void {
  const p = toArt(e);
  if (!p) return;
  const agent = agentAt(p);
  if (agent) {
    selectedAgent = selectedAgent === agent ? null : agent;
    onSelectAgent?.(selectedAgent);
    return;
  }
  if (inRect(p, FILING_CABINET)) {
    onFilingCabinetClick?.();
    return;
  }
  selectedAgent = null;
  onSelectAgent?.(selectedAgent);
}

// ── Game loop ──

function frame(time: number): void {
  if (stopped) return;
  const dt = lastTime === 0 ? 0 : Math.min((time - lastTime) / 1000, 0.1);
  lastTime = time;
  for (const hook of updateHooks) hook(dt);
  if (ctx) render(ctx, dt);
  requestAnimationFrame(frame);
}

function render(c: CanvasRenderingContext2D, dt: number): void {
  c.setTransform(1, 0, 0, 1, 0, 0);
  c.fillStyle = "#1c1820";
  c.fillRect(0, 0, c.canvas.width, c.canvas.height);
  c.setTransform(scale, 0, 0, scale, 0, 0);
  c.imageSmoothingEnabled = false;
  c.textBaseline = "alphabetic";

  const bg = getBackground();
  if (bg) c.drawImage(bg, 0, 0, ART_W, ART_H);

  for (const { hook } of renderHooks) {
    c.save();
    hook(c, dt);
    c.restore();
  }
}
