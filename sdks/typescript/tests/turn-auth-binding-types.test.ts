// Compile-time contract for the per-turn auth-binding override: the wire's
// Set/Clear tri-state, or a bare binding ref.
import type {
  DeferredTurnOptions,
  TurnAuthBindingOverride,
  TurnOptions,
} from "../src/index.js";

const setByRef: TurnOptions = {
  authBinding: { realm: "team", binding: "work-anthropic" },
};
const setExplicitly: TurnOptions = {
  authBinding: {
    action: "set",
    value: { realm: "team", binding: "work-anthropic", profile: "default" },
  },
};
const clearOnTurn: TurnOptions = { authBinding: { action: "clear" } };
const clearOnDeferredTurn: DeferredTurnOptions = { authBinding: { action: "clear" } };
// @ts-expect-error the override action is the wire's closed set/clear vocabulary.
const unknownAction: TurnAuthBindingOverride = { action: "unset" };
// @ts-expect-error a set override carries the binding ref as `value`.
const setWithoutValue: TurnAuthBindingOverride = { action: "set" };

void [setByRef, setExplicitly, clearOnTurn, clearOnDeferredTurn, unknownAction, setWithoutValue];
