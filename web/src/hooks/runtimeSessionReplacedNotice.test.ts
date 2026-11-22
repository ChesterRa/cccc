import { afterEach, expect, it, vi } from "vite-plus/test";
import i18n from "../i18n";
import { useUIStore } from "../stores";
import {
  noticeGroupSessionsReplaced,
  noticeRuntimeSessionReplaced,
} from "./runtimeSessionReplacedNotice";

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

it("names each member a Group start replaced, in one notice, and stays silent otherwise", async () => {
  const notice = vi.fn();
  useUIStore.setState({ showNotice: notice });
  await i18n.changeLanguage("en");
  const nameOf = (actorId: string) => (actorId === "rev" ? "Reviewer" : actorId);

  noticeGroupSessionsReplaced(nameOf, {
    group: {},
    runtime_sessions_replaced: [
      { actor_id: "rev", at: "2026-11-21T10:00:00Z", reasons: ["model"] },
      { actor_id: "impl", at: "2026-11-21T10:00:01Z", reasons: [] },
      { at: "2026-11-21T10:00:02Z", reasons: ["command"] },
    ],
  });

  expect(notice).toHaveBeenCalledTimes(1);
  const message = notice.mock.calls[0][0].message as string;
  expect(message).toContain("Reviewer started a new conversation");
  expect(message).toContain(i18n.t("actors:runtimeSessionReplacedReason.model"));
  expect(message).toContain("impl started a new conversation");
  expect(message).not.toContain(i18n.t("actors:runtimeSessionReplacedReason.command"));

  noticeGroupSessionsReplaced(nameOf, { group: {} });
  noticeGroupSessionsReplaced(nameOf, { runtime_sessions_replaced: [] });
  noticeGroupSessionsReplaced(nameOf, undefined);
  expect(notice).toHaveBeenCalledTimes(1);
});
