interface CapabilityMutationValidation {
  ok: boolean;
  reason?: string;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" ? (value as Record<string, unknown>) : null;
}

function readState(value: Record<string, unknown>): string {
  return String(value.state || "").trim().toLowerCase();
}

function readReason(value: Record<string, unknown> | null): string {
  if (!value) return "";
  return String(value.reason || "").trim();
}

function failFrom(value: Record<string, unknown> | null): CapabilityMutationValidation {
  const reason = readReason(value);
  return reason ? { ok: false, reason } : { ok: false };
}

export function validateCapabilityToggleResult(
  result: unknown,
  expectedEnabled: boolean
): CapabilityMutationValidation {
  const payload = asRecord(result);
  if (!payload) return { ok: false };
  if (readState(payload) !== "ready") return failFrom(payload);
  if (payload.enabled !== expectedEnabled) return failFrom(payload);
  return { ok: true };
}

export function validateCapabilityImportResult(
  result: unknown,
  opts?: { enableAfterImport?: boolean }
): CapabilityMutationValidation {
  const payload = asRecord(result);
  if (!payload) return { ok: false };
  if (readState(payload) !== "ready") return failFrom(payload);
  if (payload.imported !== true) return failFrom(payload);
  if (opts?.enableAfterImport) {
    const enableResult = asRecord(payload.enable_result);
    if (!enableResult) return failFrom(payload);
    if (readState(enableResult) !== "ready") return failFrom(enableResult);
    if (enableResult.enabled !== true) return failFrom(enableResult);
  }
  return { ok: true };
}
