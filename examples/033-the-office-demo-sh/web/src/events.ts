import type { AgentId, RuntimeModule, AgentSub } from "./types";
import { AGENTS, AGENT_IDS, CALL_COLORS } from "./types";
import { setAgentState } from "./office/characters";
import { showSpeechBubble, showThinkBubble, hideThinkBubble } from "./office/bubbles";
import { startCall, endCallsForAgent } from "./office/phonelines";

const peerAgents = new Map<string, AgentId>();
const seenEvents = new Set<string>();
const awaitingExtraction = new Set<AgentId>();
let requestSequence = 0;

export function resetEventState(): void {
  peerAgents.clear();
  seenEvents.clear();
  awaitingExtraction.clear();
  kickoffPending.clear();
  turnActions.clear();
  hostAddressed.clear();
  requestSequence = 0;
}

export async function resolvePeerAgents(mod: RuntimeModule, mobId: string): Promise<void> {
  const resolved = new Map<string, AgentId>();
  for (const id of AGENT_IDS) {
    try {
      const target = JSON.parse(String(await mod.mob_member_peer_target(mobId, id)));
      const peerId = target?.external?.peer_id;
      if (typeof peerId !== "string" || !peerId || resolved.has(peerId)) {
        throw new Error("missing or duplicate canonical peer_id");
      }
      resolved.set(peerId, id);
    } catch (error) {
      throw new Error(`Resolve peer ${id}: ${String(error)}`);
    }
  }
  peerAgents.clear();
  for (const [peer, id] of resolved) peerAgents.set(peer, id);
}

type IncidentCallback = (from: AgentId | "system", to: AgentId | null, content: string, headline: string, category: string) => void;
let onMessage: IncidentCallback | null = null;
export function setOnMessage(cb: IncidentCallback): void { onMessage = cb; }

export interface ApprovalRequest {
  request_id: string;
  short_summary?: string;
  action_description: string;
  risk_level: string;
  proposed_by: string;
}
let onApprovalNeeded: ((data: ApprovalRequest) => void) | null = null;
export function setOnApprovalNeeded(cb: (data: ApprovalRequest) => void): void { onApprovalNeeded = cb; }

// ── Startup kickoff ──
// Every autonomous member runs one kickoff turn at spawn. The host waits for
// all of them before wiring the office, so no peer is woken by kickoff
// lifecycle notices. Kickoff summaries are startup bookkeeping, not activity.

const kickoffPending = new Set<AgentId>();
export function expectKickoff(ids: readonly AgentId[]): void {
  kickoffPending.clear();
  for (const id of ids) kickoffPending.add(id);
}
export function pendingKickoffs(): AgentId[] { return [...kickoffPending]; }

// ── Per-turn activity ──
// A turn that sent nothing and changed nothing still logs its headline, but
// gets no speech bubble unless it answers a message from the human.

const VISIBLE_ACTIONS = new Set([
  "send_message", "reply_to_peer", "send_request", "send_response",
  "request_human_approval", "upsert_record", "revoke_access", "restore_access",
]);
const turnActions = new Map<AgentId, number>();
const hostAddressed = new Set<AgentId>();

/** The human just sent this agent a message; its next summary is a reply worth showing. */
export function noteHostMessage(id: AgentId): void { hostAddressed.add(id); }

function idle(id: AgentId): void {
  hideThinkBubble(id);
  endCallsForAgent(id);
  setAgentState(id, "idle");
}

/** Returns true when this terminal closed the agent's startup kickoff turn. */
function closeKickoff(id: AgentId): boolean {
  return kickoffPending.delete(id);
}

/**
 * The useful part of a runtime error for a bubble: the provider's own
 * message when one is embedded, otherwise the innermost cause.
 */
function errorGist(message: string): string {
  const provider = message.match(/"message"\s*:\s*"([^"]+)"/);
  const gist = provider ? provider[1] : message.split(": ").slice(-2).join(": ");
  return gist.length > 90 ? `${gist.slice(0, 87)}...` : gist;
}

function summary(id: AgentId, output: unknown): void {
  if (!output || typeof output !== "object") return;
  const result = output as { headline?: unknown; category?: unknown };
  if (typeof result.headline !== "string" || !result.headline) return;
  const acted = (turnActions.get(id) ?? 0) > 0;
  if (hostAddressed.delete(id) || acted) showSpeechBubble(id, result.headline, 4500);
  onMessage?.(id, null, result.headline, result.headline,
    typeof result.category === "string" ? result.category : "response");
}

export function drainAllEvents(mod: RuntimeModule, subs: AgentSub[]): { events: number; errors: string[] } {
  let events = 0;
  const errors: string[] = [];
  for (const sub of subs) {
    try {
      const parsed: any[] = JSON.parse(mod.poll_subscription(sub.handle));
      for (const event of parsed) {
        if (!event?.payload) continue;
        // Provider call IDs (notably fc_0) are reused across independent turns.
        const key = typeof event.event_id === "string"
          ? JSON.stringify([event.source?.kind, event.source?.session_id, event.event_id]) : null;
        if (key && seenEvents.has(key)) continue;
        if (key) {
          seenEvents.add(key);
          if (seenEvents.size > 4096) seenEvents.delete(seenEvents.values().next().value!);
        }
        events++;
        const p = event.payload;
        if (p.type === "tool_call_requested") {
          const args = typeof p.args === "string" ? JSON.parse(p.args) : p.args;
          if (VISIBLE_ACTIONS.has(p.name)) turnActions.set(sub.agentId, (turnActions.get(sub.agentId) ?? 0) + 1);
          if (p.name === "send_message") {
            const recipient = peerAgents.get(args.peer_id);
            const body = typeof args.body === "string" ? args.body : "";
            const preview = `Send requested: ${body.slice(0, 47)}`;
            if (recipient && body) {
              startCall(sub.agentId, recipient, recipient === "gate" ? CALL_COLORS.approval
                : recipient === "archivist" ? CALL_COLORS.knowledge : CALL_COLORS.routing);
              setAgentState(sub.agentId, "on_call");
              // The bubble wraps and ellipsizes the full body itself.
              showSpeechBubble(sub.agentId, `To ${AGENTS[recipient].name}: ${body}`, 5000);
            }
            onMessage?.(sub.agentId, recipient ?? null, body,
              recipient ? preview : "Send requested to unknown peer", "routing");
          } else if (p.name === "request_human_approval") {
            onApprovalNeeded?.({ ...args, request_id: key ?? `local-request-${requestSequence++}` });
          }
          // upsert_record, revoke_access and restore_access are host callbacks:
          // main.ts applies them and returns the real outcome to the agent.
        } else if (p.type === "run_started") {
          awaitingExtraction.delete(sub.agentId);
          turnActions.set(sub.agentId, 0);
          // Show the agent working the moment its turn starts; tool-only
          // turns may never stream text, which would leave it looking idle.
          showThinkBubble(sub.agentId);
          setAgentState(sub.agentId, "thinking");
        } else if (p.type === "run_completed") {
          idle(sub.agentId);
          if (p.extraction_required) {
            awaitingExtraction.add(sub.agentId);
          } else {
            awaitingExtraction.delete(sub.agentId);
            let output = p.structured_output;
            if (!output && p.result) {
              try { output = JSON.parse(p.result); } catch { /* Plain text is not a headline object. */ }
            }
            if (!closeKickoff(sub.agentId)) summary(sub.agentId, output);
          }
        } else if (p.type === "extraction_succeeded") {
          idle(sub.agentId);
          if (awaitingExtraction.delete(sub.agentId) && !closeKickoff(sub.agentId)) {
            summary(sub.agentId, p.structured_output);
          }
        } else if (p.type === "run_failed" || p.type === "extraction_failed") {
          idle(sub.agentId);
          awaitingExtraction.delete(sub.agentId);
          closeKickoff(sub.agentId);
          const message = p.error_report?.message ?? p.reason ?? "Unspecified runtime failure";
          errors.push(`${sub.agentId}: ${message}`);
          showSpeechBubble(sub.agentId, `Error: ${errorGist(message)}`, 8000);
        } else if (p.type === "text_delta") {
          showThinkBubble(sub.agentId);
          setAgentState(sub.agentId, "thinking");
        } else if (p.type === "tool_execution_started") {
          setAgentState(sub.agentId, "on_call");
        } else if (p.type === "stream_truncated") {
          const message = p.reason?.kind === "stream_lagged"
            ? `Event stream lost ${p.reason.dropped} events; host tool effects may be missing. Restart the office.`
            : "Event stream truncated; restart the office.";
          errors.push(`${sub.agentId}: ${message}`);
          onMessage?.("system", sub.agentId, message, message, "error");
        }
      }
    } catch (error) {
      errors.push(`${sub.agentId}: Event polling/handling failed: ${String(error)}`);
    }
  }
  return { events, errors };
}
