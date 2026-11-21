import { afterEach, expect, it, vi } from "vite-plus/test";
import i18n from "../i18n";
import { useUIStore } from "../stores";
import { noticeRuntimeSessionReplaced } from "./runtimeSessionReplacedNotice";

const uiBefore = useUIStore.getState();

afterEach(async () => {
  useUIStore.setState(uiBefore, true);
  await i18n.changeLanguage("en");
});

it("says once, with the reasons it knows, that a start began a new conversation", async () => {
  const notice = vi.fn();
  useUIStore.setState({ showNotice: notice });
  await i18n.changeLanguage("en");

  noticeRuntimeSessionReplaced("Reviewer", {
    runtime_session_replaced: {
      at: "2026-11-21T04:26:48Z",
      reasons: ["command", "model", "a_future_reason"],
    },
  });

  expect(notice).toHaveBeenCalledTimes(1);
  const message = notice.mock.calls[0][0].message as string;
  expect(message).toContain("Reviewer started a new conversation");
  expect(message).toContain(i18n.t("actors:runtimeSessionReplacedReason.command"));
  expect(message).toContain(i18n.t("actors:runtimeSessionReplacedReason.model"));
  expect(message).not.toContain("a_future_reason");
});

it("stays silent when the start resumed the saved conversation or the result is not an object", () => {
  const notice = vi.fn();
  useUIStore.setState({ showNotice: notice });

  noticeRuntimeSessionReplaced("Reviewer", { actor: { id: "reviewer" } });
  noticeRuntimeSessionReplaced("Reviewer", undefined);
  noticeRuntimeSessionReplaced("Reviewer", "unexpected");

  expect(notice).not.toHaveBeenCalled();
});
