// Compile-time contract: a deferred session's first turn takes the same
// per-turn host options as any other turn, and a mob member turn takes the
// typed auth-binding override.
import type { DeferredTurnOptions, MobTurnStartOptions } from "../src/index.js";

const deferredHostOptions: DeferredTurnOptions = {
  selfHostedServerId: "local-gpu",
  injectedContext: ["ambient host context"],
  transientTurnContext: "request-only host facts",
};
const mobClear: MobTurnStartOptions = { authBinding: { action: "clear" } };
const mobRef: MobTurnStartOptions = {
  authBinding: { realm: "team", binding: "work-anthropic" },
};
// @ts-expect-error the override action is the wire's closed set/clear vocabulary.
const mobUnknownAction: MobTurnStartOptions = { authBinding: { action: "unset" } };

void [deferredHostOptions, mobClear, mobRef, mobUnknownAction];
