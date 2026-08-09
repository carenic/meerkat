import { accessSync, constants } from "node:fs";
import path from "node:path";

/**
 * Resolve the runtime used by explicit TypeScript SDK smoke lanes.
 *
 * Smoke tests must never guess from the workspace or PATH. Their caller owns
 * the build and passes the exact binary under test, which prevents an older
 * installed rkat-rpc from masquerading as the current release candidate.
 */
export function resolveRequiredSmokeBinary(env = process.env) {
  const configured =
    env.MEERKAT_BIN_PATH?.trim() || env.MEERKAT_RPC_BINARY?.trim();
  if (!configured) {
    throw new Error(
      "TypeScript SDK smoke tests require MEERKAT_BIN_PATH or MEERKAT_RPC_BINARY to name the exact rkat-rpc binary under test",
    );
  }

  const resolved = path.resolve(configured);
  try {
    accessSync(resolved, constants.X_OK);
  } catch {
    throw new Error(
      `TypeScript SDK smoke binary is missing or not executable: ${resolved}`,
    );
  }
  return resolved;
}
