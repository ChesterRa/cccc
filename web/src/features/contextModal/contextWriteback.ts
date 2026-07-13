import type { ApiResponse } from "../../services/api";

function makeReadbackErrorResponse<T>(error: unknown): ApiResponse<T> {
  const message =
    error instanceof Error ? error.message : String(error || "context readback failed");
  return {
    ok: false,
    error: { code: "context_readback_failed", message: message || "context readback failed" },
  };
}

export async function reloadContextAfterWrite<T>(
  response: ApiResponse<T>,
  reloadContext: () => Promise<void>,
): Promise<ApiResponse<T>> {
  if (!response.ok) {
    return response;
  }
  try {
    await reloadContext();
  } catch (error) {
    return makeReadbackErrorResponse<T>(error);
  }
  return response;
}
