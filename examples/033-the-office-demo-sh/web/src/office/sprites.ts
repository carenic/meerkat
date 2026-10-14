// ═══════════════════════════════════════════════════════════
// Sprites - pixel-art assets and the geometry derived from them
//   office/bg.png            640x360 room shell
//   office/chars/<id>.png    4 frames: typing A, typing B, phone, thinking
//   office/desks/<id>.png    desk, anchor = bottom-centre
//   office/props/*.png       phone, phone_off, envelope, folder
// ═══════════════════════════════════════════════════════════

import { AGENT_IDS } from "../types";
import type { AgentId } from "../types";
import { CHAR_BACK, DESKS } from "./layout";
import { pixelFont } from "./pixel";

export const Frame = {
  TypingA: 0,
  TypingB: 1,
  Phone: 2,
  Thinking: 3,
} as const;
export type Frame = (typeof Frame)[keyof typeof Frame];

const FRAME_COUNT = 4;

export interface CharSprite {
  img: HTMLImageElement;
  frameW: number;
  frameH: number;
}

export type PropName = "phone" | "phone_off" | "envelope" | "folder";

let background: HTMLImageElement | null = null;
const chars = new Map<AgentId, CharSprite>();
const desks = new Map<AgentId, HTMLImageElement>();
const props = new Map<PropName, HTMLImageElement>();

// Fallback sizes keep geometry stable before the art has loaded.
const DEFAULT_CHAR_H = 60;
const DEFAULT_DESK = { w: 66, h: 43 };

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`Failed to load ${src}`));
    img.src = src;
  });
}

export async function loadSprites(base = "./office"): Promise<void> {
  const jobs: Promise<void>[] = [
    loadImage(`${base}/bg.png`).then((img) => { background = img; }),
  ];
  // Canvas text only uses a web font once it has loaded; a failure falls back.
  void document.fonts?.load(pixelFont(7)).catch(() => undefined);
  for (const id of AGENT_IDS) {
    jobs.push(loadImage(`${base}/chars/${id}.png`).then((img) => {
      chars.set(id, { img, frameW: img.width / FRAME_COUNT, frameH: img.height });
    }));
    jobs.push(loadImage(`${base}/desks/${id}.png`).then((img) => { desks.set(id, img); }));
  }
  for (const name of ["phone", "phone_off", "envelope", "folder"] as const) {
    jobs.push(loadImage(`${base}/props/${name}.png`).then((img) => { props.set(name, img); }));
  }
  const results = await Promise.allSettled(jobs);
  const failed = results.filter((r) => r.status === "rejected").length;
  if (failed > 0) throw new Error(`${failed} office sprite(s) failed to load`);
}

export function getBackground(): HTMLImageElement | null { return background; }
export function getChar(id: AgentId): CharSprite | undefined { return chars.get(id); }
export function getDesk(id: AgentId): HTMLImageElement | undefined { return desks.get(id); }
export function getProp(name: PropName): HTMLImageElement | undefined { return props.get(name); }

// ── Geometry derived from sprite sizes (all in art pixels) ──

function deskSize(id: AgentId): { w: number; h: number } {
  const img = desks.get(id);
  return img ? { w: img.width, h: img.height } : DEFAULT_DESK;
}

/** Top-centre of the character's head. */
export function headPoint(id: AgentId): { x: number; y: number } {
  const d = DESKS[id];
  const h = chars.get(id)?.frameH ?? DEFAULT_CHAR_H;
  return { x: d.x, y: d.y - CHAR_BACK - h + 2 };
}

/** Where the desk phone sits on the desktop (its bottom-left corner). */
export function phoneSlot(id: AgentId): { x: number; y: number } {
  const d = DESKS[id];
  const size = deskSize(id);
  const phoneW = props.get("phone")?.width ?? 14;
  return { x: d.x + Math.floor(size.w / 2) - phoneW - 5, y: d.y - size.h + 15 };
}

/** Centre of the desk phone, for call lines. */
export function phonePoint(id: AgentId): { x: number; y: number } {
  const slot = phoneSlot(id);
  const img = props.get("phone");
  return { x: slot.x + (img ? img.width / 2 : 7), y: slot.y - (img ? img.height / 2 : 6) };
}

/** A spot on the left of the desktop where incoming mail lands. */
export function inTrayPoint(id: AgentId): { x: number; y: number } {
  const d = DESKS[id];
  const size = deskSize(id);
  return { x: d.x - Math.floor(size.w / 2) + 12, y: d.y - size.h + 10 };
}

/** Clickable body of an agent: chair, torso and head down to the desk front. */
export function agentHitRect(id: AgentId): { x: number; y: number; w: number; h: number } {
  const d = DESKS[id];
  const size = deskSize(id);
  const head = headPoint(id);
  return { x: d.x - size.w / 2, y: head.y, w: size.w, h: d.y - head.y };
}
