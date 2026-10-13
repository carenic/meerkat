import type {
  ConsentNoticeKind,
  ConsentNoticeParams,
  ConsentPresentationParams,
} from "./generated/types.js";

/** The notification carrying one recorded R3 human consent transition. */
export const CONSENT_NOTICE_NOTIFICATION = "approval/consent_notice";

/**
 * One `approval/consent_notice`, as `parseConsentNotice` narrows it. A
 * `requested` notice carries the approval's presentation: what its approver
 * is asked to decide, as the approval owner recorded it.
 */
export type ConsentNotice = Omit<ConsentNoticeParams, "presentation"> & {
  presentation?: ConsentPresentationParams;
};

/** Receives each `approval/consent_notice` as its typed params. */
export type ConsentNoticeListener = (notice: ConsentNotice) => void;

const CONSENT_NOTICE_KINDS: ReadonlySet<string> = new Set<ConsentNoticeKind>([
  "requested",
  "approved",
  "denied",
  "used",
  "withdrawn",
]);

const APPROVAL_RISKS: ReadonlySet<string> = new Set<ConsentPresentationParams["risk"]>([
  "low",
  "medium",
  "high",
  "critical",
]);

const CONSENT_ASSURANCES: ReadonlySet<string> = new Set<
  ConsentPresentationParams["accepted_assurance"]
>(["device_trusted", "person_authenticated"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isOptionalString(value: unknown): value is string | null | undefined {
  return value === undefined || value === null || typeof value === "string";
}

/**
 * Narrow one notice's presentation. Returns `undefined` for a malformed one
 * or one naming a risk or an assurance this SDK build does not know.
 */
function parseConsentPresentation(value: unknown): ConsentPresentationParams | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const { action, risk, account, mandate, accepted_assurance, expires_at } = value;
  if (
    !isRecord(action) ||
    typeof action.kind !== "string" ||
    typeof action.summary !== "string" ||
    typeof risk !== "string" ||
    !APPROVAL_RISKS.has(risk) ||
    typeof accepted_assurance !== "string" ||
    !CONSENT_ASSURANCES.has(accepted_assurance) ||
    typeof expires_at !== "string" ||
    !isOptionalString(account) ||
    !isOptionalString(mandate)
  ) {
    return undefined;
  }
  return {
    action,
    risk: risk as ConsentPresentationParams["risk"],
    accepted_assurance: accepted_assurance as ConsentPresentationParams["accepted_assurance"],
    expires_at,
    ...(typeof account === "string" ? { account } : {}),
    ...(typeof mandate === "string" ? { mandate } : {}),
  };
}

/**
 * Narrow one `approval/consent_notice` payload to its typed params. Returns
 * `undefined` for a malformed payload, a transition kind this SDK build does
 * not know, or a presentation it cannot read, so a newer server's notices
 * are ignored, never misread. A notice grants nothing: an `approved` one
 * executes nothing by itself.
 */
export function parseConsentNotice(params: unknown): ConsentNotice | undefined {
  if (!isRecord(params)) {
    return undefined;
  }
  const { approval_id, tool_call_id, kind, presentation } = params;
  if (
    typeof approval_id !== "string" ||
    typeof tool_call_id !== "string" ||
    typeof kind !== "string" ||
    !CONSENT_NOTICE_KINDS.has(kind)
  ) {
    return undefined;
  }
  const notice: ConsentNotice = { approval_id, tool_call_id, kind: kind as ConsentNoticeKind };
  if (presentation === undefined || presentation === null) {
    return notice;
  }
  const parsed = parseConsentPresentation(presentation);
  return parsed === undefined ? undefined : { ...notice, presentation: parsed };
}
