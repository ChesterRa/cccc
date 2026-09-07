// Production voice controls with synthetic HTTP only: no microphone or provider calls.
import { useState } from "react";
import i18next from "../../src/i18n";
import {
  VoiceSecretaryComposerControl,
  type VoiceSecretaryCaptureMode,
} from "../../src/pages/chat/VoiceSecretaryComposerControl";
import "../../src/index.css";

window.fetch = async () =>
  Response.json({
    ok: true,
    result: {
      assistant: {
        assistant_id: "voice_secretary",
        enabled: true,
        config: { recognition_language: "mixed" },
      },
      documents: [],
      sessions: [],
    },
  });

export function Fixture() {
  const [group, setGroup] = useState("g1");
  const [disabled, setDisabled] = useState(false);
  const [mode, setMode] = useState<VoiceSecretaryCaptureMode>("prompt");
  const [target, setTarget] = useState<HTMLDivElement | null>(null);
  Object.assign(window, {
    voiceMobileProbe: {
      setGroup,
      setDisabled,
      language: (value: string) => i18next.changeLanguage(value),
    },
  });
  return (
    <main style={{ padding: 16 }}>
      <p>Voice controls · {group}</p>
      <div ref={setTarget} />
      <textarea aria-label="Draft" defaultValue="Please review the implementation." />
      <VoiceSecretaryComposerControl
        isDark={false}
        selectedGroupId={group}
        busy=""
        disabled={disabled}
        variant="assistantRow"
        captureMode={mode}
        onCaptureModeChange={setMode}
        composerText="Please review the implementation."
        statusPortalTarget={target}
      />
    </main>
  );
}
