import {
  SUPPORTED_RUNTIMES,
  isWebModelRuntime,
  type RuntimeInfo,
  type SupportedRuntime,
} from "../types";

/**
 * The runtime a new member's form should show: the current one when it is installed, else the
 * first installed command-line runtime, so that someone with only Claude Code does not land on a
 * disabled Add. Null when none is.
 *
 * Availability alone does not say what the reader installed: web models and custom commands are
 * always available, and DeepSeek is available wherever Node and npm are, since CCCC can install
 * its harness. None of them is chosen here.
 */
export function installedRuntimeFor(
  current: SupportedRuntime,
  runtimes: RuntimeInfo[],
): SupportedRuntime | null {
  const installed = (name: string) => runtimes.some((item) => item.name === name && item.available);
  if (installed(current)) return current;
  return (
    SUPPORTED_RUNTIMES.find(
      (name) =>
        name !== "custom" && name !== "deepseek" && !isWebModelRuntime(name) && installed(name),
    ) ?? null
  );
}
