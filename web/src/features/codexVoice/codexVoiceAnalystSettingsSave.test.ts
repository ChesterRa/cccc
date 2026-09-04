import { beforeEach, expect, it, vi } from "vite-plus/test";
import { saveVoiceAnalystSettingsWithConsent } from "./codexVoiceAnalystSettingsSave";

const updateSettings = vi.hoisted(() => vi.fn());

vi.mock("../../services/api", () => ({ updateCodexVoiceAnalystSettings: updateSettings }));

const request = {
  settings: {
    runtime: "opencode",
    command: "",
    profile_id: "",
    profile_scope: "global" as const,
    profile_owner: "",
  },
  environmentSet: {},
  environmentUnset: [],
  environmentClear: false,
};

beforeEach(() => updateSettings.mockReset());

it("stops known Analyst work and applies settings after one explicit confirmation", async () => {
  updateSettings.mockResolvedValue({
    ok: true,
    result: { analyst: null, restarted: true, started_new_session: true, discarded_work: true },
  });
  const confirm = vi.fn(() => true);

  const outcome = await saveVoiceAnalystSettingsWithConsent({
    request,
    analystBusy: true,
    identityConfirmationRequired: true,
    confirm,
    discardConfirmation: "discard work",
    identityConfirmation: "change identity",
  });

  expect(outcome.cancelled).toBe(false);
  expect(confirm).toHaveBeenCalledOnce();
  expect(confirm).toHaveBeenCalledWith("discard work");
  expect(updateSettings).toHaveBeenCalledWith({ ...request, discardCurrentWork: true });
});

it("recovers a queued-work race by confirming and retrying the same settings transaction", async () => {
  updateSettings
    .mockResolvedValueOnce({
      ok: false,
      error: { code: "codex_voice_settings_busy", message: "queued", details: {} },
    })
    .mockResolvedValueOnce({
      ok: true,
      result: { analyst: null, restarted: true, started_new_session: false, discarded_work: true },
    });
  const confirm = vi.fn(() => true);

  const outcome = await saveVoiceAnalystSettingsWithConsent({
    request,
    analystBusy: false,
    identityConfirmationRequired: false,
    confirm,
    discardConfirmation: "discard work",
    identityConfirmation: "change identity",
  });

  expect(outcome.cancelled).toBe(false);
  expect(confirm).toHaveBeenCalledWith("discard work");
  expect(updateSettings).toHaveBeenNthCalledWith(1, { ...request, discardCurrentWork: false });
  expect(updateSettings).toHaveBeenNthCalledWith(2, { ...request, discardCurrentWork: true });
});

it("does not stop work or change settings when the user declines", async () => {
  const outcome = await saveVoiceAnalystSettingsWithConsent({
    request,
    analystBusy: true,
    identityConfirmationRequired: false,
    confirm: () => false,
    discardConfirmation: "discard work",
    identityConfirmation: "change identity",
  });

  expect(outcome.cancelled).toBe(true);
  expect(updateSettings).not.toHaveBeenCalled();
});
