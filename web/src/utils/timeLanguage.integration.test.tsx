// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import i18next from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { expect, it, vi } from "vite-plus/test";
import { WorkingSummaryPanel } from "../components/ContextModal/coordination/WorkingSummaryPanel";
import { createContextModalUi } from "../components/ContextModal/ui";
import { HeadlessRawTrace } from "../components/headless/HeadlessRawTrace";
import { InboxModal } from "../components/modals/InboxModal";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it("updates timestamps in the summary, Inbox and headless trace when the UI language changes", async () => {
  const language = i18next.createInstance();
  await language
    .use(initReactI18next)
    .init({
      lng: "en",
      fallbackLng: "en",
      ns: ["common", "modals", "actors"],
      defaultNS: "modals",
      resources: {
        en: { common: {}, modals: {}, actors: {} },
        zh: { common: {}, modals: {}, actors: {} },
        ja: { common: {}, modals: {}, actors: {} },
      },
      interpolation: { escapeValue: false },
    });
  const at = new Date(Date.now() - 5 * 60 * 1000).toISOString();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () =>
      root.render(
        <I18nextProvider i18n={language}>
          <div data-area="summary">
            <WorkingSummaryPanel
              tr={(key, fallback, values) =>
                (fallback || key).replace("{{time}}", String(values?.time || ""))
              }
              ui={createContextModalUi(false)}
              brief={{ objective: "A", project_brief: "Current summary", updated_at: at }}
              tasksSummary={{}}
              attentionCounts={{ blocked: 0, waitingUser: 0, pendingHandoffs: 0 }}
              unassignedCount={0}
              editingBrief={false}
              briefDraft={{
                objective: "",
                currentFocus: "",
                projectBrief: "",
                constraints: "",
                projectBriefStale: false,
              }}
              syncBusy={false}
              onStartBriefEdit={vi.fn()}
              onCancelBriefEdit={vi.fn()}
              onSaveBrief={vi.fn()}
              onBriefDraftChange={vi.fn()}
            />
          </div>
          <div data-area="inbox">
            <InboxModal
              isOpen
              isDark={false}
              actorId="actor"
              actors={[]}
              messages={[
                { id: "m1", kind: "chat.message", by: "user", ts: at, data: { text: "Hello" } },
              ]}
              busy=""
              onClose={vi.fn()}
              onMarkAllRead={vi.fn()}
            />
          </div>
          <div data-area="trace">
            <HeadlessRawTrace
              events={[
                {
                  id: "e1",
                  ts: at,
                  type: "headless.message.delta",
                  data: { stream_id: "s", delta: "Hello" },
                },
              ]}
              emptyLabel="empty"
              isDark={false}
            />
          </div>
        </I18nextProvider>,
      ),
    );
    // The summary's timestamp is inside the expandable working summary.
    await act(async () =>
      host.querySelector<HTMLButtonElement>("[data-goal-summary-key]")!.click(),
    );
    const check = (expected: string) => {
      for (const area of ["summary", "inbox", "trace"]) {
        expect(host.querySelector(`[data-area="${area}"]`)?.textContent).toContain(expected);
      }
    };
    check("5m ago");
    await act(async () => language.changeLanguage("zh"));
    check("5分钟前");
    await act(async () => language.changeLanguage("ja"));
    check("5分前");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
