import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft } from "lucide-react";
import { LcdIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { DirectInviteLifetime } from "./DirectInviteLifetime";
import { DirectJoinForm, DirectReceiveForm } from "./DirectPairingForms";
import { readDirectInvitation, directInvitationExpired } from "./directConnectionModel";
import type { useDirectActions } from "./useDirectActions";

export function DirectConnectionTask({
  groupTitle,
  actions,
}: {
  groupTitle: string;
  actions: ReturnType<typeof useDirectActions>;
}) {
  const { t } = useTranslation("layout");
  const [expiresInSeconds, setExpiresInSeconds] = useState(604800);
  const { task, taskHeading, busy, status, incoming, setIncoming, closeTask, setError, act } =
    actions;
  if (!task) return null;
  const reusable = !!status?.capabilities?.reusable_invitations;
  return (
    <section className="space-y-4 rounded-xl border border-[var(--glass-border-subtle)] p-4">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <Button
          size="sm"
          variant="ghost"
          className="-ml-3 min-h-11 shrink-0 gap-1"
          disabled={busy}
          onClick={closeTask}
        >
          <LcdIcon icon={ChevronLeft} glyph="chevron-left" className="h-4 w-4" aria-hidden="true" />
          {t("direct.back")}
        </Button>
        <h3 ref={taskHeading} tabIndex={-1} className="text-base font-semibold outline-none">
          {t(
            task === "invite"
              ? "direct.inviteGroup"
              : task === "join"
                ? "direct.useInvite"
                : "direct.configure",
          )}
        </h3>
      </div>
      {task === "join" ? (
        <DirectJoinForm
          groupTitle={groupTitle}
          incoming={incoming}
          setIncoming={setIncoming}
          busy={busy}
          onJoin={() => {
            const preview = readDirectInvitation(incoming);
            if (!preview || directInvitationExpired(preview.expires_at)) {
              setError(t("direct.expiredHint"));
              return;
            }
            if (preview.permanent && !status?.capabilities?.permanent_invitations) {
              setError(t("direct.permanentUpgrade"));
              return;
            }
            if (preview.reusable && !reusable) {
              setError(t("direct.reusableUpgrade"));
              return;
            }
            void act("join", { invitation: incoming.trim() });
          }}
        />
      ) : (
        <>
          {task === "invite" && (
            <DirectInviteLifetime
              value={expiresInSeconds}
              onChange={setExpiresInSeconds}
              disabled={busy || !reusable}
            />
          )}
          {task === "invite" &&
            expiresInSeconds === 0 &&
            !status?.capabilities?.permanent_invitations && (
              <p role="status" className="text-[var(--color-text-secondary)]">
                {t("direct.permanentUpgrade")}
              </p>
            )}
          <DirectReceiveForm
            key={task}
            listener={status?.listener || null}
            addresses={status?.addresses || []}
            inviteMode={task === "invite"}
            name={status?.display_name || ""}
            busy={busy}
            unavailable={
              task === "invite" &&
              (!reusable ||
                (expiresInSeconds === 0 && !status?.capabilities?.permanent_invitations))
            }
            onSave={(listener, name, previous) =>
              void act(task === "invite" ? "invite" : "configure", {
                listener,
                display_name: name,
                expected_listener: previous,
                ...(task === "invite" ? { expires_in_seconds: expiresInSeconds } : {}),
              })
            }
            onStop={() =>
              void act("configure", { listener: null, expected_listener: status?.listener })
            }
          />
          {busy && (
            <p role="status">{t(task === "invite" ? "direct.preparing" : "direct.saving")}</p>
          )}
          {status?.runtime?.error && (
            <p className="break-words text-[var(--color-danger)]" role="alert">
              {t("direct.listenerFailed")} {status.runtime.error}
            </p>
          )}
        </>
      )}
    </section>
  );
}
