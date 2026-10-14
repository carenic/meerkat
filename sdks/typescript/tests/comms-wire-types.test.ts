// Compile-time contract: the hand-written comms/send types follow the
// generated wire contracts.
import type { CommsCommand } from "../src/index.js";

type LifecycleKind = Extract<CommsCommand, { kind: "peer_lifecycle" }>["lifecycle_kind"];

const dismiss: LifecycleKind = "mob.dismiss";
// @ts-expect-error lifecycle kinds are the generated closed vocabulary.
const unknownLifecycle: LifecycleKind = "mob.unknown";

void [dismiss, unknownLifecycle];
