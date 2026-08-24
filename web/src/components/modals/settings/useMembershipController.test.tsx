// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../../../services/api";
import { useMembershipController } from "./useMembershipController";

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

describe("useMembershipController", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
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
});
