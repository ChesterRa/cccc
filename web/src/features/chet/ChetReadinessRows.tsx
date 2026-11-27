import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CloseIcon } from "../../components/Icons";
import { IconButton } from "../../components/ui/icon-button";
import type { CodexVoiceReadiness } from "../../services/api";

const LOGIN_REMINDER_DISMISSED = "cccc.chet.voiceLoginReminder.dismissed";

function loginReminderDismissed(): boolean {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem(LOGIN_REMINDER_DISMISSED) === "1";
  } catch {
    return false;
  }
}

/** Optional voice setup is dismissible; runtime readiness remains independent. */
export function ChetReadinessRows({
  readiness,
  callProblem,
  analystProblem,
}: {
  readiness: CodexVoiceReadiness | null;
  callProblem: string;
  analystProblem: string;
}) {
  const { t } = useTranslation("modals");
  const [dismissed, setDismissed] = useState(loginReminderDismissed);
  const missingChatGptLogin =
    readiness !== null &&
    readiness.realtime_provider !== "grok" &&
    !readiness.realtime_credentials_available;
  const dismissLoginReminder = () => {
    setDismissed(true);
    try {
      window.localStorage.setItem(LOGIN_REMINDER_DISMISSED, "1");
    } catch {
      // As with other browser preferences, this page still keeps the choice.
    }
  };

  return (
    <dl className="min-w-0 flex-1 basis-full space-y-1 leading-6 sm:basis-0">
      {[
        {
          label: "codexVoiceReadinessCall",
          problem: callProblem,
          dismissible: missingChatGptLogin,
        },
        { label: "chet.runtime", problem: analystProblem, dismissible: false },
      ].map(({ label, problem, dismissible }) =>
        dismissible && dismissed ? null : (
          <div
            key={label}
            className={`relative block min-w-0 sm:flex sm:gap-2 ${dismissible ? "pr-12" : ""}`}
          >
            <dt className="mr-2 inline font-semibold text-[var(--color-text-primary)] sm:mr-0 sm:block sm:flex-none">
              {t(label)}
            </dt>
            <dd
              className={`inline min-w-0 break-words sm:block ${
                problem ? "text-amber-700 dark:text-amber-300" : "text-[var(--color-text-muted)]"
              }`}
            >
              {problem || t("codexVoiceReadinessOk")}
              {dismissible ? (
                <IconButton
                  type="button"
                  variant="ghost"
                  size="sm"
                  label={t("codexVoiceDismissLoginReminder")}
                  className="absolute -top-1 right-0 min-h-9 min-w-9 max-sm:min-h-11 max-sm:min-w-11"
                  onClick={dismissLoginReminder}
                >
                  <CloseIcon size={14} aria-hidden="true" />
                </IconButton>
              ) : null}
            </dd>
          </div>
        ),
      )}
    </dl>
  );
}
