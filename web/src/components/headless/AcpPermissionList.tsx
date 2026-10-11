import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { AcpPermission, AcpInteractionResponse } from "../../services/api/codexVoice";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";

export function AcpPermissionList({
  permissions,
  disabled,
  onRespond,
  onInteract,
}: {
  permissions: AcpPermission[];
  disabled?: boolean;
  onRespond(requestId: string, allow: boolean): void;
  onInteract(requestId: string, response: AcpInteractionResponse): void;
}) {
  const { t } = useTranslation("actors");
  return (
    <div className="space-y-3">
      {permissions.map((request) =>
        ["pi_select", "pi_confirm", "pi_input", "pi_editor"].includes(request.kind) ? (
          <PiInteraction
            key={request.request_id}
            request={request}
            disabled={disabled}
            onInteract={onInteract}
            onConfirm={onRespond}
          />
        ) : request.kind === "question" || request.kind === "plan" ? (
          <UserInteraction
            key={request.request_id}
            request={request}
            disabled={disabled}
            onRespond={onInteract}
          />
        ) : (
          <div
            key={request.request_id}
            className="rounded-lg border border-amber-500/25 p-3 text-sm"
          >
            <p className="text-xs text-[var(--color-text-muted)]">{t("acpControls.permission")}</p>
            <p className="mt-1 break-words">{request.title || request.kind}</p>
            {request.details != null && (
              <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-all text-xs">
                {typeof request.details === "string"
                  ? request.details
                  : JSON.stringify(request.details, null, 2)}
              </pre>
            )}
            <div className="mt-2 flex flex-wrap gap-2">
              <Button
                size="sm"
                disabled={disabled}
                onClick={() => onRespond(request.request_id, true)}
              >
                {t("acpControls.allow")}
              </Button>
              <Button
                size="sm"
                variant="secondary"
                disabled={disabled}
                onClick={() => onRespond(request.request_id, false)}
              >
                {t("acpControls.deny")}
              </Button>
            </div>
          </div>
        ),
      )}
    </div>
  );
}

function PiInteraction({
  request,
  disabled,
  onInteract,
  onConfirm,
}: {
  request: AcpPermission;
  disabled?: boolean;
  onInteract(requestId: string, response: AcpInteractionResponse): void;
  onConfirm(requestId: string, allow: boolean): void;
}) {
  const { t } = useTranslation("actors");
  const [value, setValue] = useState(request.kind === "pi_select" ? "" : request.prefill || "");
  const [selected, setSelected] = useState(false);
  return (
    <div className="rounded-lg border border-amber-500/25 p-3 text-sm">
      <p className="break-words font-medium">{request.title}</p>
      {request.message && <p className="mt-2 whitespace-pre-wrap break-words">{request.message}</p>}
      {request.kind === "pi_confirm" ? (
        <div className="mt-3 flex flex-wrap gap-2">
          <Button size="sm" disabled={disabled} onClick={() => onConfirm(request.request_id, true)}>
            {t("acpControls.confirm")}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            disabled={disabled}
            onClick={() => onConfirm(request.request_id, false)}
          >
            {t("acpControls.decline")}
          </Button>
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (!disabled && (request.kind !== "pi_select" || selected))
              onInteract(request.request_id, { value });
          }}
        >
          {request.kind === "pi_select" ? (
            <fieldset className="mt-3 space-y-2" disabled={disabled}>
              <legend className="sr-only">{request.title}</legend>
              {(request.options || []).map((option) => (
                <label key={option} className="flex min-h-9 cursor-pointer items-start gap-2 py-1">
                  <input
                    className="mt-1 shrink-0"
                    type="radio"
                    name={request.request_id}
                    checked={selected && value === option}
                    onChange={() => {
                      setValue(option);
                      setSelected(true);
                    }}
                  />
                  <span className="min-w-0 break-words">{option}</span>
                </label>
              ))}
            </fieldset>
          ) : request.kind === "pi_editor" ? (
            <Textarea
              className="mt-3 min-h-28"
              aria-label={request.title}
              placeholder={request.placeholder}
              value={value}
              maxLength={65536}
              disabled={disabled}
              onChange={(event) => setValue(event.target.value)}
            />
          ) : (
            <Input
              className="mt-3"
              type="text"
              aria-label={request.title}
              placeholder={request.placeholder}
              value={value}
              maxLength={65536}
              disabled={disabled}
              onChange={(event) => setValue(event.target.value)}
            />
          )}
          <div className="mt-3 flex flex-wrap gap-2">
            <Button
              size="sm"
              type="submit"
              disabled={disabled || (request.kind === "pi_select" && !selected)}
            >
              {t("acpControls.reply")}
            </Button>
            <Button
              size="sm"
              type="button"
              variant="secondary"
              disabled={disabled}
              onClick={() => onInteract(request.request_id, { cancelled: true })}
            >
              {t("acpControls.cancelInteraction")}
            </Button>
          </div>
        </form>
      )}
    </div>
  );
}

function UserInteraction({
  request,
  disabled,
  onRespond,
}: {
  request: AcpPermission;
  disabled?: boolean;
  onRespond(requestId: string, response: AcpInteractionResponse): void;
}) {
  const { t } = useTranslation("actors");
  const [choices, setChoices] = useState<Record<string, string[]>>({});
  const questions = request.questions || [];
  const answered =
    questions.length > 0 && questions.every((question) => choices[question.id]?.length);
  const reply = (outcome: "accepted" | "rejected" | "skipped" | "cancelled") =>
    onRespond(request.request_id, { outcome: { outcome } });
  return (
    <div className="rounded-lg border border-amber-500/25 p-3 text-sm">
      <p className="text-xs text-[var(--color-text-muted)]">
        {t(request.kind === "question" ? "acpControls.question" : "acpControls.plan")}
      </p>
      {request.title && <p className="mt-1 break-words font-medium">{request.title}</p>}
      {request.kind === "question" ? (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (answered && !disabled)
              onRespond(request.request_id, {
                outcome: {
                  outcome: "answered",
                  answers: questions.map((question) => ({
                    questionId: question.id,
                    selectedOptionIds: choices[question.id],
                  })),
                },
              });
          }}
        >
          {questions.map((question) => (
            <fieldset key={question.id} disabled={disabled} className="mt-3 space-y-2">
              <legend className="break-words">{question.prompt}</legend>
              {question.options.map((option) => (
                <label
                  key={option.id}
                  className="flex min-h-9 cursor-pointer items-start gap-2 py-1"
                >
                  <input
                    className="mt-1 shrink-0"
                    type={question.allowMultiple ? "checkbox" : "radio"}
                    name={`${request.request_id}:${question.id}`}
                    checked={choices[question.id]?.includes(option.id) || false}
                    onChange={(event) => {
                      const selected = event.target.checked;
                      setChoices((old) => ({
                        ...old,
                        [question.id]: question.allowMultiple
                          ? selected
                            ? [...(old[question.id] || []), option.id]
                            : (old[question.id] || []).filter((id) => id !== option.id)
                          : [option.id],
                      }));
                    }}
                  />
                  <span className="min-w-0 break-words">{option.label}</span>
                </label>
              ))}
            </fieldset>
          ))}
          <div className="mt-3 flex flex-wrap gap-2">
            <Button size="sm" type="submit" disabled={disabled || !answered}>
              {t("acpControls.answer")}
            </Button>
            <Button
              size="sm"
              type="button"
              variant="secondary"
              disabled={disabled}
              onClick={() => reply("skipped")}
            >
              {t("acpControls.skip")}
            </Button>
          </div>
        </form>
      ) : (
        <>
          {request.overview && (
            <p className="mt-2 whitespace-pre-wrap break-words">{request.overview}</p>
          )}
          <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-words font-sans text-sm">
            {request.plan}
          </pre>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button size="sm" disabled={disabled} onClick={() => reply("accepted")}>
              {t("acpControls.acceptPlan")}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={disabled}
              onClick={() => reply("rejected")}
            >
              {t("acpControls.rejectPlan")}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
