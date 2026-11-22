import { beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { fetchChetUnattended, updateChetUnattended } from "../services/api/chet";
import { useUnattendedStore } from "./useUnattendedStore";

vi.mock("../services/api/chet", () => ({
  fetchChetUnattended: vi.fn(),
  updateChetUnattended: vi.fn(),
}));

describe("useUnattendedStore", () => {
  beforeEach(() => {
    useUnattendedStore.setState({ unattended: null, saving: false, error: "" });
    vi.mocked(fetchChetUnattended).mockReset();
    vi.mocked(updateChetUnattended).mockReset();
  });

  it("reads the instance setting and keeps it on a failed read", async () => {
    vi.mocked(fetchChetUnattended).mockResolvedValueOnce({
      ok: true,
      result: { enabled: true, since: "2026-11-22T14:00:00Z" },
    });
    await useUnattendedStore.getState().refresh();
    expect(useUnattendedStore.getState().unattended).toEqual({
      enabled: true,
      since: "2026-11-22T14:00:00Z",
    });
    vi.mocked(fetchChetUnattended).mockResolvedValueOnce({
      ok: false,
      error: { code: "io_error", message: "down" },
    });
    await useUnattendedStore.getState().refresh();
    expect(useUnattendedStore.getState().unattended?.enabled).toBe(true);
  });

  it("shows what the host stored, and keeps the old state with the error when a change fails", async () => {
    vi.mocked(updateChetUnattended).mockResolvedValueOnce({
      ok: true,
      result: { enabled: true, since: "2026-11-22T14:00:00Z" },
    });
    await useUnattendedStore.getState().setEnabled(true);
    expect(updateChetUnattended).toHaveBeenCalledWith(true);
    expect(useUnattendedStore.getState()).toMatchObject({
      unattended: { enabled: true },
      saving: false,
      error: "",
    });
    vi.mocked(updateChetUnattended).mockResolvedValueOnce({
      ok: false,
      error: { code: "permission_denied", message: "denied" },
    });
    await useUnattendedStore.getState().setEnabled(false);
    expect(useUnattendedStore.getState()).toMatchObject({
      unattended: { enabled: true },
      saving: false,
      error: "denied",
    });
  });

  it("ignores a read that started before a change or returns while one is in flight", async () => {
    let answerRead: (value: Awaited<ReturnType<typeof fetchChetUnattended>>) => void = () => {};
    vi.mocked(fetchChetUnattended).mockImplementationOnce(
      () => new Promise((resolve) => (answerRead = resolve)),
    );
    const read = useUnattendedStore.getState().refresh();
    vi.mocked(updateChetUnattended).mockResolvedValueOnce({
      ok: true,
      result: { enabled: true, since: "2026-11-22T14:00:00Z" },
    });
    await useUnattendedStore.getState().setEnabled(true);
    answerRead({ ok: true, result: { enabled: false, since: null } });
    await read;
    expect(useUnattendedStore.getState().unattended?.enabled).toBe(true);
    // A read after the change is applied as usual.
    vi.mocked(fetchChetUnattended).mockResolvedValueOnce({
      ok: true,
      result: { enabled: false, since: null },
    });
    await useUnattendedStore.getState().refresh();
    expect(useUnattendedStore.getState().unattended?.enabled).toBe(false);
  });
});
