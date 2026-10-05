import { isWebModelRuntime, supportsAcpMode, type RuntimeInfo, type RuntimeMode } from "../types";
import type { GroupState } from "../stores/groupStoreTypes";

export function runtimeDetectionKey(
  runtime: string,
  info: RuntimeInfo | undefined,
  status: GroupState["runtimeDetectionStatus"],
  mode: RuntimeMode = "default",
): string {
  if (isWebModelRuntime(runtime)) return "web";
  if (runtime === "custom") return "custom";
  if (status === "idle" || status === "loading") return "checking";
  if (status === "error") return "failed";
  const available =
    supportsAcpMode(runtime) && mode === "acp" ? info?.mode_availability?.acp : info?.available;
  if (available === undefined) return "unknown";
  return available ? "detected" : "missing";
}
