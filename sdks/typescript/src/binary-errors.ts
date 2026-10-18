import { MeerkatError } from "./generated/errors.js";

/** What a {@link BinaryIntegrityError} refused. */
export interface BinaryIntegrityFailure {
  /** The release archive the binary comes from. */
  readonly asset: string;
  /** The cached binary that failed, or `null` for a download. */
  readonly path: string | null;
  readonly expectedSha256: string;
  readonly actualSha256: string;
}

/**
 * A downloaded or cached `rkat-rpc` does not match its release digest.
 *
 * Thrown with code `BINARY_INTEGRITY_MISMATCH` when the auto-downloaded
 * release archive, the binary inside it, or the cached binary does not match
 * the sha256 this package pins (stamped at release time). The SDK removes
 * what failed, never runs it and never falls back to another binary.
 * `details` carries the same fields in wire spelling.
 */
export class BinaryIntegrityError extends MeerkatError {
  readonly asset: string;
  readonly path: string | null;
  readonly expectedSha256: string;
  readonly actualSha256: string;

  constructor(message: string, failure: BinaryIntegrityFailure) {
    super("BINARY_INTEGRITY_MISMATCH", message, {
      asset: failure.asset,
      path: failure.path,
      expected_sha256: failure.expectedSha256,
      actual_sha256: failure.actualSha256,
    });
    this.name = "BinaryIntegrityError";
    this.asset = failure.asset;
    this.path = failure.path;
    this.expectedSha256 = failure.expectedSha256;
    this.actualSha256 = failure.actualSha256;
  }
}
