// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { GroupBridgePairingSection } from "./GroupBridgePairingSection";
import * as api from "../../../services/api";
import { copyTextToClipboard } from "../../../utils/copy";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

vi.mock("../../../services/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../services/api")>();
  return {
    ...original,
    createGroupBridgePairingInvite: vi.fn(),
    createGroupBridgePairingConnectionInfo: vi.fn(),
  };
});

vi.mock("../../../utils/copy", () => ({ copyTextToClipboard: vi.fn() }));

const payload = {
  code: "2816-3245",
  issuer_endpoint: "http://localhost:5555",
  nonce: "pairing-nonce",
  version: 2,
};
const expectedInvite = JSON.stringify(payload, null, 2);
const refreshPairing = vi.fn(async (): Promise<void> => undefined);

describe("GroupBridgePairingSection invitation auto-copy", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.createGroupBridgePairingInvite).mockResolvedValue({
      ok: true,
      result: {
        invite: {
          invite_id: "invite-1",
          group_id: "group-a",
          remote_group_id: "",
          remote_peer_id: "",
          transport: "session",
          status: "pending",
          expires_at: "2026-08-07T10:00:00Z",
        },
      },
    });
    vi.mocked(api.createGroupBridgePairingConnectionInfo).mockResolvedValue({
      ok: true,
      result: { payload },
    });
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    await act(async () => {
      root.render(
        <GroupBridgePairingSection
          isDark={false}
          currentGroupId="group-a"
          currentGroupTitle="Group A"
          identity={null}
          requests={[]}
          trusts={[]}
          outbounds={[]}
          refreshPairing={refreshPairing}
        />,
      );
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  it("copies the complete invitation immediately after generation", async () => {
    vi.mocked(copyTextToClipboard).mockResolvedValue(true);

    await generateInvite();

    expect(copyTextToClipboard).toHaveBeenCalledWith(expectedInvite);
    expect(container.querySelector("pre")?.textContent).toBe(expectedInvite);
    expect(container.textContent).toContain("group_bridge.copyConnectionInfoDone");
    expect(refreshPairing).toHaveBeenCalledOnce();
  });

  it("keeps the invitation visible when automatic copy is unavailable", async () => {
    vi.mocked(copyTextToClipboard).mockResolvedValue(false);

    await generateInvite();

    expect(container.querySelector("pre")?.textContent).toBe(expectedInvite);
    expect(container.textContent).toContain("group_bridge.copyConnectionInfoManual");
    expect(refreshPairing).toHaveBeenCalledOnce();
  });

  async function generateInvite() {
    const button = [...container.querySelectorAll("button")].find(
      (candidate) => candidate.textContent === "group_bridge.createConnectionInfo",
    );
    expect(button).toBeTruthy();
    await act(async () => button?.click());
  }
});
