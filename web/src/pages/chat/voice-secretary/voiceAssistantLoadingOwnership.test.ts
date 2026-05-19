import { describe, expect, it } from "vitest";
import {
  beginVoiceAssistantRefresh,
  resetVoiceAssistantVisibleLoading,
  shouldApplyVoiceAssistantRefresh,
  shouldFinishVoiceAssistantVisibleLoading,
  type VoiceAssistantLoadingOwnership,
} from "./voiceAssistantLoadingOwnership";

function initialOwnership(): VoiceAssistantLoadingOwnership {
  return { dataSeq: 0, visibleLoadingSeq: 0 };
}

describe("voice assistant loading ownership", () => {
  it("lets the original visible refresh clear loading after a quiet refresh resolves first", () => {
    let ownership = initialOwnership();
    const visible = beginVoiceAssistantRefresh(ownership, { quiet: false });
    ownership = visible.next;
    const quiet = beginVoiceAssistantRefresh(ownership, { quiet: true });
    ownership = quiet.next;

    expect(shouldApplyVoiceAssistantRefresh(ownership, quiet.request, true)).toBe(true);
    expect(shouldFinishVoiceAssistantVisibleLoading(ownership, quiet.request, true)).toBe(false);
    expect(shouldApplyVoiceAssistantRefresh(ownership, visible.request, true)).toBe(false);
    expect(shouldFinishVoiceAssistantVisibleLoading(ownership, visible.request, true)).toBe(true);
  });

  it("lets the original visible refresh clear loading after a quiet refresh resolves later", () => {
    let ownership = initialOwnership();
    const visible = beginVoiceAssistantRefresh(ownership, { quiet: false });
    ownership = visible.next;
    const quiet = beginVoiceAssistantRefresh(ownership, { quiet: true });
    ownership = quiet.next;

    expect(shouldApplyVoiceAssistantRefresh(ownership, visible.request, true)).toBe(false);
    expect(shouldFinishVoiceAssistantVisibleLoading(ownership, visible.request, true)).toBe(true);
    expect(shouldApplyVoiceAssistantRefresh(ownership, quiet.request, true)).toBe(true);
    expect(shouldFinishVoiceAssistantVisibleLoading(ownership, quiet.request, true)).toBe(false);
  });

  it("clears visible loading for ok:false or thrown visible refreshes", () => {
    const visible = beginVoiceAssistantRefresh(initialOwnership(), { quiet: false });

    expect(shouldFinishVoiceAssistantVisibleLoading(visible.next, visible.request, true)).toBe(true);
  });

  it("clears loading on group switch and prevents old requests from owning new group UI", () => {
    const visible = beginVoiceAssistantRefresh(initialOwnership(), { quiet: false });
    const afterSwitch = resetVoiceAssistantVisibleLoading(visible.next);

    expect(afterSwitch.visibleLoadingSeq).toBe(0);
    expect(shouldApplyVoiceAssistantRefresh(afterSwitch, visible.request, false)).toBe(false);
    expect(shouldFinishVoiceAssistantVisibleLoading(afterSwitch, visible.request, false)).toBe(false);
    expect(shouldFinishVoiceAssistantVisibleLoading(afterSwitch, visible.request, true)).toBe(false);
  });
});
