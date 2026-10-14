// ═══════════════════════════════════════════════════════════
// Office Layout - art-space geometry
// Everything in the office is authored on a 640x360 pixel-art
// grid; the canvas scales that grid to the display.
// Desk anchor = bottom-centre of the desk's front panel.
// ═══════════════════════════════════════════════════════════

import type { AgentId } from "../types";

export const ART_W = 640;
export const ART_H = 360;

export interface DeskPos {
  x: number;
  y: number;
  label: string;
  zone: string;
}

export const DESKS: Record<AgentId, DeskPos> = {
  triage:     { x: 110, y: 150, label: "Triage",     zone: "Mail Room" },
  archivist:  { x: 530, y: 150, label: "Archivist",  zone: "Archive" },
  "it-dept":  { x: 62,  y: 245, label: "IT Support", zone: "Dept Row" },
  "hr-dept":  { x: 168, y: 245, label: "HR",         zone: "Dept Row" },
  "alex-pa":  { x: 452, y: 245, label: "PA Alex",    zone: "PA Bullpen" },
  "sam-pa":   { x: 528, y: 245, label: "PA Sam",     zone: "PA Bullpen" },
  "pat-pa":   { x: 604, y: 245, label: "PA Pat",     zone: "PA Bullpen" },
  facilities: { x: 62,  y: 340, label: "Facilities", zone: "Dept Row" },
  finance:    { x: 168, y: 340, label: "Finance",    zone: "Dept Row" },
  gate:       { x: 320, y: 340, label: "Compliance", zone: "Compliance" },
};

/** How far behind (above) the desk anchor the seated character's chair base sits. */
export const CHAR_BACK = 18;

/** Wall-mounted mail box in the Mail Room. */
export const MAIL_SLOT = { x: 26, y: 26 };

/** Filing cabinets + archive shelf; clicking opens the Records tab. */
export const FILING_CABINET = { x: 440, y: 2, w: 172, h: 66 };
/** Where filed folders disappear into. */
export const CABINET_DRAWER = { x: 477, y: 34 };

export interface ZoneSign {
  label: string;
  x: number;
  y: number;
}

/**
 * Zone plaques for the two rooms with interactive fixtures (the mail
 * slot and the records cabinet). Desk nameplates label everyone else.
 */
export const ZONE_SIGNS: ZoneSign[] = [
  { label: "MAIL ROOM", x: 182, y: 80 },
  { label: "ARCHIVE", x: 616, y: 80 },
];

/** Sunlight falling through the three windows (dust motes drift here). */
export const SUNBEAM = { x: 236, y: 52, w: 180, h: 110 };
