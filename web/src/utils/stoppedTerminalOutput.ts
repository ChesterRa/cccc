import { stripInactiveTerminalWorkingBanners } from "./terminalWorkingState";

export function getStoppedTerminalOutputText(text: string, workingState?: string): string {
  return stripInactiveTerminalWorkingBanners(String(text || ""), workingState).trim();
}
