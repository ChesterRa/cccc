import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { NativeSessionTerminal } from "../voice/NativeSessionTerminal";
import type { CodexVoiceAnalystInfo } from "../../services/api";
import { getCodexVoiceTerminalWebSocketUrl } from "../../services/api";

type Props = { analyst: CodexVoiceAnalystInfo; isVisible: boolean; runtime?: string };

export function VoiceAnalystTerminal({ analyst, isVisible, runtime }: Props) {
  const { t } = useTranslation("modals");
  const buildWebSocketUrl = useCallback(
    (query: string) => getCodexVoiceTerminalWebSocketUrl(analyst.generation, query),
    [analyst.generation],
  );
  return (
    <NativeSessionTerminal
      generation={analyst.generation}
      scopeId="codex-voice"
      isVisible={isVisible}
      runtime={runtime}
      buildWebSocketUrl={buildWebSocketUrl}
      lifecycleHint={t("codexVoiceTerminalLifecycleHint")}
      rejectedHint={t("codexVoiceTerminalRejected")}
      reconnectHint={t("codexVoiceTerminalReconnectHint")}
    />
  );
}
