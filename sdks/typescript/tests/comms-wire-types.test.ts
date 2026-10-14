// Compile-time contract: the hand-written comms/send types follow the
// generated wire contracts.
import type {
  CommsCommand,
  CommsSendReceipt,
  PeerDeliveryOutcome,
  PeerIngressTerminalOutcomeKind,
} from "../src/index.js";

type LifecycleKind = Extract<CommsCommand, { kind: "peer_lifecycle" }>["lifecycle_kind"];

const dismiss: LifecycleKind = "mob.dismiss";
// @ts-expect-error lifecycle kinds are the generated closed vocabulary.
const unknownLifecycle: LifecycleKind = "mob.unknown";

void [dismiss, unknownLifecycle];

// The comms/send receipt is the snake_case wire result, with no camelCase copy.
declare const receipt: CommsSendReceipt;
if (receipt.kind === "peer_request_sent") {
  const requestId: string = receipt.request_id;
  void requestId;
}
// @ts-expect-error the SDK never populated camelCase receipt fields.
void receipt.requestId;

// Every peer receipt's delivery is the named, fully typed PeerDeliveryOutcome.
type Delivery = Extract<CommsSendReceipt, { kind: "peer_message_sent" }>["delivery"];
const durable: Delivery = { durably_resolved: { outcome: "deduplicated" } };
const named: PeerDeliveryOutcome = durable;
const outcome: PeerIngressTerminalOutcomeKind = "rejected";
// @ts-expect-error terminal ingress outcomes are the generated closed vocabulary.
const unknownOutcome: Delivery = { durably_resolved: { outcome: "dropped" } };
void [named, outcome, unknownOutcome];
