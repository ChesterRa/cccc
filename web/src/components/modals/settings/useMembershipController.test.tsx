// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../../../services/api";
import { useMembershipController } from "./useMembershipController";
import type { MembershipState } from "../../../types";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  const i18n = { language: "zh", resolvedLanguage: "zh-CN" };
  return { useTranslation: () => ({ t, i18n }) };
});

vi.mock("../../../services/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../services/api")>();
  return {
    ...original,
    fetchMembership: vi.fn(),
    startMembershipLogin: vi.fn(),
    pollMembershipLogin: vi.fn(),
    logoutMembership: vi.fn(),
    startMembershipReach: vi.fn(),
    stopMembershipReach: vi.fn(),
  };
});

function Probe() {
  const controller = useMembershipController(true);
  return (
    <button
      type="button"
      disabled={controller.membershipBusy}
      onClick={() => void controller.connect()}
    >
      connect
    </button>
  );
}

function PollingProbe() {
  const controller = useMembershipController(true);
  return <output data-ready={String(controller.membershipPollReady)} />;
}

let reachController: ReturnType<typeof useMembershipController>;
function ReachProbe({ active = true }: { active?: boolean }) {
  reachController = useMembershipController(active);
  return <output data-checking={String(reachController.reachChecking)} />;
}

const activeReach: MembershipState = {
  logged_in: true,
  in_reach: true,
  reach_enabled: true,
  reach_status: "offline",
  online: false,
  cloudflared: { running: true },
};

describe("useMembershipController", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    vi.clearAllMocks();
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.fetchMembership).mockResolvedValue({
      ok: true,
      result: { membership: { logged_in: false } },
    });
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("opens the approval tab in the user gesture before login completes", async () => {
    let resolveLogin:
      | ((value: Awaited<ReturnType<typeof api.startMembershipLogin>>) => void)
      | undefined;
    vi.mocked(api.startMembershipLogin).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveLogin = resolve;
        }),
    );
    const replace = vi.fn();
    const close = vi.fn();
    const popup = { opener: null, location: { replace }, close } as unknown as Window;
    const open = vi.spyOn(window, "open").mockReturnValue(popup);

    await act(async () => root.render(<Probe />));
    const button = container.querySelector("button");
    expect(button?.disabled).toBe(false);

    act(() => button?.click());
    expect(open).toHaveBeenCalledWith("about:blank", "_blank");
    expect(api.startMembershipLogin).toHaveBeenCalledOnce();
    expect(replace).not.toHaveBeenCalled();

    await act(async () => {
      resolveLogin?.({
        ok: true,
        result: {
          membership: {
            logged_in: false,
            account_origin: "https://account.example.test/",
            pending: {
              user_code: "ABCD-EFGH",
              verification_uri_complete: "https://account.example.test/device?user_code=ABCD-EFGH",
              interval: 5,
            },
          },
        },
      });
      await Promise.resolve();
    });

    expect(replace).toHaveBeenCalledWith(
      "https://account.example.test/device?user_code=ABCD-EFGH&lang=zh",
    );
    expect(close).not.toHaveBeenCalled();
  });

  it("backs off after a transient polling failure", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-24T00:00:00Z"));
    vi.mocked(api.fetchMembership).mockResolvedValue({
      ok: true,
      result: {
        membership: { logged_in: false, pending: { user_code: "ABCD-EFGH", interval: 1 } },
      },
    });
    vi.mocked(api.pollMembershipLogin).mockRejectedValue(new Error("offline"));

    await act(async () => {
      root.render(<PollingProbe />);
      await Promise.resolve();
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_025);
    });
    expect(api.pollMembershipLogin).toHaveBeenCalledOnce();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_025);
    });
    expect(api.pollMembershipLogin).toHaveBeenCalledOnce();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1_000);
    });
    expect(api.pollMembershipLogin).toHaveBeenCalledTimes(2);
  });

  async function beginReachCheck() {
    vi.useFakeTimers();
    vi.mocked(api.startMembershipReach).mockResolvedValue({
      ok: true,
      result: { membership: { ...activeReach, reach_status: "connecting" } },
    });
    await act(async () => root.render(<ReachProbe />));
    vi.mocked(api.fetchMembership).mockResolvedValue({
      ok: true,
      result: { membership: activeReach },
    });
    await act(async () => {
      await reachController.startReach();
    });
  }

  async function expireReachCheck() {
    await beginReachCheck();
    for (let n = 0; n < 7; n++) {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(5_000);
      });
    }
    expect(reachController.reachCheckExpired).toBe(true);
  }

  it.each<[string, MembershipState]>([
    ["connected", { ...activeReach, online: true, reach_status: "online" }],
    [
      "stopped",
      {
        ...activeReach,
        reach_enabled: false,
        reach_status: "off",
        cloudflared: { running: false },
      },
    ],
    ["cut", { ...activeReach, cut: true, reach_status: "off" }],
    ["unlinked", { logged_in: false }],
    ["helper exited", { ...activeReach, cloudflared: { running: false } }],
  ])("clears an expired check when manual refresh confirms %s", async (_label, membership) => {
    await expireReachCheck();
    vi.mocked(api.fetchMembership).mockResolvedValue({ ok: true, result: { membership } });
    await act(async () => {
      await reachController.refresh();
    });
    expect(reachController.membership).toEqual(membership);
    expect(reachController.reachChecking).toBe(false);
    expect(reachController.reachCheckExpired).toBe(false);
    const calls = vi.mocked(api.fetchMembership).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(api.fetchMembership).toHaveBeenCalledTimes(calls);
    expect(api.startMembershipReach).toHaveBeenCalledOnce();
  });

  it("retains the expired warning when refresh cannot confirm a terminal state", async () => {
    await expireReachCheck();
    for (const membership of [activeReach, { ...activeReach, reach_status: "unknown" as const }]) {
      vi.mocked(api.fetchMembership).mockResolvedValue({ ok: true, result: { membership } });
      await act(async () => {
        await reachController.refresh();
      });
      expect(reachController.reachCheckExpired).toBe(true);
    }
    vi.mocked(api.fetchMembership).mockRejectedValue(new Error("unavailable"));
    await act(async () => {
      await reachController.refresh();
    });
    expect(reachController.reachCheckExpired).toBe(true);
    expect(reachController.reachChecking).toBe(false);
  });

  it("cancels pending automatic checks when manual refresh confirms a connection", async () => {
    await beginReachCheck();
    vi.mocked(api.fetchMembership).mockResolvedValue({
      ok: true,
      result: { membership: { ...activeReach, online: true, reach_status: "online" } },
    });
    await act(async () => {
      await reachController.refresh();
    });
    expect(reachController.reachChecking).toBe(false);
    expect(reachController.reachCheckExpired).toBe(false);
    const calls = vi.mocked(api.fetchMembership).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(api.fetchMembership).toHaveBeenCalledTimes(calls);
  });

  it("confirms a started tunnel with GETs and stops checking when connected", async () => {
    await beginReachCheck();
    expect(reachController.reachChecking).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(reachController.membership?.online).toBe(false);
    vi.mocked(api.fetchMembership).mockResolvedValue({
      ok: true,
      result: { membership: { ...activeReach, online: true, reach_status: "online" } },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(reachController.reachChecking).toBe(false);
    expect(reachController.membership?.online).toBe(true);
    const calls = vi.mocked(api.fetchMembership).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(api.fetchMembership).toHaveBeenCalledTimes(calls);
    expect(api.startMembershipReach).toHaveBeenCalledOnce();
  });

  it("ends unsuccessful confirmation without restarting Reach and supports a manual retry", async () => {
    await beginReachCheck();
    for (let n = 0; n < 7; n++) {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(5_000);
      });
    }
    expect(api.fetchMembership).toHaveBeenCalledTimes(7); // Initial load + six checks.
    expect(reachController.reachChecking).toBe(false);
    expect(reachController.reachCheckExpired).toBe(true);
    expect(reachController.membership?.reach_enabled).toBe(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(api.fetchMembership).toHaveBeenCalledTimes(7);
    await act(async () => {
      reachController.checkReach();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(api.fetchMembership).toHaveBeenCalledTimes(8);
    expect(api.startMembershipReach).toHaveBeenCalledOnce();
  });

  it("allows stopping between checks and cancels the rest of the confirmation", async () => {
    await beginReachCheck();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    vi.mocked(api.stopMembershipReach).mockResolvedValue({
      ok: true,
      result: {
        membership: {
          ...activeReach,
          reach_enabled: false,
          reach_status: "off",
          cloudflared: { running: false },
        },
      },
    });
    await act(async () => {
      expect(await reachController.stopReach()).toBe(true);
    });
    const calls = vi.mocked(api.fetchMembership).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(api.fetchMembership).toHaveBeenCalledTimes(calls);
    expect(reachController.reachChecking).toBe(false);
    expect(reachController.reachCheckExpired).toBe(false);
    expect(reachController.membership?.reach_status).toBe("off");
  });

  it("ends confirmation promptly when the helper has exited", async () => {
    await beginReachCheck();
    vi.mocked(api.fetchMembership).mockResolvedValue({
      ok: true,
      result: { membership: { ...activeReach, cloudflared: { running: false } } },
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(reachController.reachChecking).toBe(false);
    expect(reachController.membership?.reach_status).toBe("offline");
    expect(api.startMembershipReach).toHaveBeenCalledOnce();
  });

  it("pauses hidden-page checks and expires without a burst when the page returns", async () => {
    await beginReachCheck();
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    const calls = vi.mocked(api.fetchMembership).mock.calls.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(api.fetchMembership).toHaveBeenCalledTimes(calls);
    hidden.mockReturnValue(false);
    act(() => document.dispatchEvent(new Event("visibilitychange")));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(reachController.reachCheckExpired).toBe(true);
    expect(api.fetchMembership).toHaveBeenCalledTimes(calls);
  });

  it("bounds a stalled status request so connection checking cannot stay busy forever", async () => {
    await beginReachCheck();
    vi.mocked(api.fetchMembership).mockImplementation(
      (signal) =>
        new Promise((_resolve, reject) => {
          signal?.addEventListener(
            "abort",
            () => reject(new DOMException("Aborted", "AbortError")),
            { once: true },
          );
        }),
    );
    for (let n = 0; n < 12; n++) {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(5_000);
      });
    }
    expect(reachController.membershipBusy).toBe(false);
    expect(reachController.reachChecking).toBe(false);
    expect(reachController.reachCheckExpired).toBe(true);
    expect(api.fetchMembership).toHaveBeenCalledTimes(2); // Initial load + the stalled check.
    expect(api.startMembershipReach).toHaveBeenCalledOnce();
  });

  it("ignores a late status response after leaving the panel", async () => {
    let resolve: (value: Awaited<ReturnType<typeof api.fetchMembership>>) => void = () => {};
    vi.mocked(api.fetchMembership).mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    await act(async () => root.render(<ReachProbe />));
    await act(async () => root.render(<ReachProbe active={false} />));
    await act(async () => {
      resolve({ ok: true, result: { membership: activeReach } });
    });
    expect(reachController.membership).toBeNull();
    expect(api.startMembershipReach).not.toHaveBeenCalled();
  });
});
