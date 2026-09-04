import { updateCodexVoiceAnalystSettings } from "../../services/api";

type SettingsUpdate = Omit<
  Parameters<typeof updateCodexVoiceAnalystSettings>[0],
  "discardCurrentWork"
>;

export async function saveVoiceAnalystSettingsWithConsent(args: {
  request: SettingsUpdate;
  analystBusy: boolean;
  identityConfirmationRequired: boolean;
  confirm(message: string): boolean;
  discardConfirmation: string;
  identityConfirmation: string;
}) {
  let discardCurrentWork = args.analystBusy;
  if (discardCurrentWork) {
    if (!args.confirm(args.discardConfirmation)) return { cancelled: true } as const;
  } else if (args.identityConfirmationRequired && !args.confirm(args.identityConfirmation)) {
    return { cancelled: true } as const;
  }

  let response = await updateCodexVoiceAnalystSettings({ ...args.request, discardCurrentWork });
  // The Runtime can accept native queued input between the last status poll and this write.
  // Reuse the same explicit consent path instead of surfacing a stale, unactionable busy error.
  if (!response.ok && response.error.code === "codex_voice_settings_busy" && !discardCurrentWork) {
    if (!args.confirm(args.discardConfirmation)) return { cancelled: true } as const;
    discardCurrentWork = true;
    response = await updateCodexVoiceAnalystSettings({ ...args.request, discardCurrentWork });
  }
  return { cancelled: false, response } as const;
}
