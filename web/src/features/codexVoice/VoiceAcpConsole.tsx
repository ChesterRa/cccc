import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  controlCodexVoiceAnalyst,
  fetchActiveCodexVoiceCall,
  type CodexVoiceAnalystInfo,
  type CodexVoiceCallInfo,
} from "../../services/api/codexVoice";
import { AcpPermissionList } from "../../components/headless/AcpPermissionList";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/textarea";
import { ChevronDownIcon } from "../../components/Icons";
import { LazyMarkdownRenderer } from "../../components/LazyMarkdownRenderer";

const RESULT_TEXT_CLASS = "whitespace-pre-wrap break-words font-sans text-sm leading-6";

function AcpResultText({ content, isDark }: { content: string; isDark?: boolean }) {
  return (
    <LazyMarkdownRenderer
      content={content}
      isDark={isDark}
      className="text-sm leading-6"
      fallback={<pre className={RESULT_TEXT_CLASS}>{content}</pre>}
    />
  );
}

export function VoiceAcpConsole({
  analyst,
  visible,
  call,
  isDark,
  onAnalystSnapshot,
}: {
  analyst: CodexVoiceAnalystInfo;
  visible: boolean;
  call: CodexVoiceCallInfo | null;
  isDark?: boolean;
  onAnalystSnapshot: (analyst: CodexVoiceAnalystInfo) => void;
}) {
  const { t } = useTranslation("actors");
  const [current, setCurrent] = useState(analyst);
  const [text, setText] = useState("");
  const [inputId, setInputId] = useState(() => crypto.randomUUID());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const outputRef = useRef<HTMLDivElement>(null);
  const controlRevision = useRef(0);
  // Call events carry phase/output, but not permissions or the full queue.
  // Keep that snapshot current while a call uses this Analyst, even collapsed.
  const pollActive = visible || call?.analyst_generation === analyst.generation;
  const submissionScope = useRef<{ inputId: string; callGeneration: string | null } | null>(null);
  const callGeneration =
    call?.connected && call.mode === "assistant" && call.analyst_generation === analyst.generation
      ? call.generation
      : null;
  const tasks = current.manual_tasks || [];
  const latestTaskId = tasks.at(-1)?.id;
  useEffect(() => {
    if (outputRef.current) outputRef.current.scrollTop = outputRef.current.scrollHeight;
  }, [latestTaskId]);
  useEffect(() => {
    const input = inputRef.current;
    if (!input) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 144)}px`;
  }, [text]);
  useEffect(() => {
    // Socket updates carry phase/output, not the authoritative manual history.
    setCurrent((previous) => ({
      ...analyst,
      ...(previous.manual_tasks
        ? { manual_tasks: previous.manual_tasks, manual_task_id: previous.manual_task_id }
        : {}),
    }));
  }, [analyst]);
  useEffect(() => {
    if (!pollActive) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      const revision = controlRevision.current;
      try {
        const response = await fetchActiveCodexVoiceCall();
        if (
          alive &&
          revision === controlRevision.current &&
          response.ok &&
          response.result.analyst?.generation === analyst.generation
        ) {
          setCurrent(response.result.analyst);
          onAnalystSnapshot(response.result.analyst);
        }
      } catch {
        /* Keep the last snapshot if a read fails. */
      }
      if (alive) timer = setTimeout(() => void poll(), 1200);
    };
    void poll();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [pollActive, analyst.generation, onAnalystSnapshot]);
  async function control(command: Parameters<typeof controlCodexVoiceAnalyst>[1]) {
    if (busy) return;
    controlRevision.current += 1;
    setBusy(true);
    setError("");
    try {
      const response = await controlCodexVoiceAnalyst(analyst.generation, command);
      if (!response.ok) {
        setError(response.error.message);
        return;
      }
      if (response.result.analyst?.generation === analyst.generation) {
        setCurrent(response.result.analyst);
        onAnalystSnapshot(response.result.analyst);
      }
      if (command.action === "input") {
        setText("");
        setInputId(crypto.randomUUID());
        submissionScope.current = null;
      }
    } catch {
      setError(t("acpControls.error"));
    } finally {
      controlRevision.current += 1;
      setBusy(false);
    }
  }
  function submit() {
    if (busy || !text.trim()) return;
    // An uncertain HTTP response must not silently retarget the retry to a
    // replacement call. Editing the draft constitutes an explicit new input.
    if (submissionScope.current?.inputId !== inputId)
      submissionScope.current = { inputId, callGeneration };
    void control({
      action: "input",
      input_id: inputId,
      text: text.trim(),
      call_generation: submissionScope.current.callGeneration,
    });
  }
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div ref={outputRef} className="min-h-0 flex-1 space-y-4 overflow-auto px-5 py-4">
        <p className="text-xs text-[var(--color-text-muted)]">{t("acpControls.hint")}</p>
        {!!current.queued_inputs && (
          <p role="status" className="text-xs">
            {t("acpControls.queued", { count: current.queued_inputs })}
          </p>
        )}
        {current.last_error && !current.manual_task_id && (
          <div role="alert" className="space-y-1 text-sm text-rose-600 dark:text-rose-400">
            <p>{t("acpControls.turnFailed")}</p>
            <p className="whitespace-pre-wrap break-words">{current.last_error}</p>
            {(current.progress || current.last_result) && (
              <p className="text-xs">{t("acpControls.partialOutput")}</p>
            )}
          </div>
        )}
        {!current.manual_task_id && (current.progress || current.last_result) && (
          <AcpResultText content={current.progress || current.last_result || ""} isDark={isDark} />
        )}
        {!!tasks.length && (
          <div className="space-y-4" aria-label={t("acpControls.investigations")}>
            <h4 className="text-xs font-medium text-[var(--color-text-muted)]">
              {t("acpControls.investigations")}
            </h4>
            {tasks.map((task, index) => (
              <details
                key={task.id}
                open={index === tasks.length - 1 || ["queued", "working"].includes(task.status)}
                className="group space-y-2 border-b border-[var(--glass-border-subtle)] pb-4 last:border-0"
              >
                <summary className="flex cursor-pointer list-none items-start justify-between gap-3 text-sm">
                  <ChevronDownIcon
                    className="mt-0.5 h-3.5 w-3.5 shrink-0 -rotate-90 text-[var(--color-text-muted)] group-open:rotate-0"
                    aria-hidden="true"
                  />
                  <span className="min-w-0 flex-1 truncate group-open:whitespace-pre-wrap group-open:break-words">
                    {task.text}
                  </span>
                  <span className="shrink-0 pt-0.5 text-xs text-[var(--color-text-muted)]">
                    {t(
                      `acpControls.taskStatus.${["queued", "working", "completed", "cancelled", "unconfirmed"].includes(task.status) ? task.status : "failed"}`,
                    )}
                  </span>
                </summary>
                <div className="space-y-2 pt-2">
                  {task.status === "cancelled" && task.result && (
                    <p className="text-xs text-[var(--color-text-muted)]">
                      {t("acpControls.partialOutput")}
                    </p>
                  )}
                  {!["queued", "working", "completed", "cancelled"].includes(task.status) && (
                    <p
                      role="alert"
                      className="break-words text-xs text-rose-600 dark:text-rose-400"
                    >
                      {t(
                        task.status === "unconfirmed"
                          ? "acpControls.outcomeUnconfirmed"
                          : "acpControls.turnFailed",
                      )}{" "}
                      {task.error || task.status}
                      {task.result ? ` ${t("acpControls.partialOutput")}` : ""}
                    </p>
                  )}
                  {task.result && <AcpResultText content={task.result} isDark={isDark} />}
                  <p className="text-xs text-[var(--color-text-muted)]">
                    {t(
                      !task.call_generation
                        ? "acpControls.localResult"
                        : task.call_generation === callGeneration
                          ? "acpControls.callResult"
                          : "acpControls.endedCallResult",
                    )}
                  </p>
                </div>
              </details>
            ))}
          </div>
        )}
        <AcpPermissionList
          key={analyst.generation}
          permissions={current.permissions || []}
          onInteract={(request_id, response) =>
            void control({ action: "interaction", request_id, response })
          }
          disabled={busy}
          onRespond={(request_id, allow) =>
            void control({ action: "permission", request_id, allow })
          }
        />
      </div>
      <form
        className="flex-none space-y-2 border-t border-[var(--glass-border-subtle)] p-3"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <p className="text-xs text-[var(--color-text-muted)]">
          {t(
            submissionScope.current && submissionScope.current.callGeneration !== callGeneration
              ? "acpControls.retryScopeHint"
              : callGeneration
                ? "acpControls.currentCallHint"
                : "acpControls.localHint",
          )}
        </p>
        <div className="glass-input flex min-w-0 items-end gap-2 rounded-xl p-2 focus-within:!border-[var(--color-border-focus)] focus-within:!shadow-[var(--focus-ring)]">
          <Textarea
            ref={inputRef}
            className="min-h-16 max-h-36 min-w-0 flex-1 resize-none !border-0 !bg-transparent px-1 py-1 !shadow-none !outline-none !ring-0"
            value={text}
            onChange={(event) => {
              setText(event.target.value);
              setInputId(crypto.randomUUID());
              submissionScope.current = null;
              setError("");
            }}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                (event.ctrlKey || event.metaKey) &&
                !event.nativeEvent.isComposing
              ) {
                event.preventDefault();
                submit();
              }
            }}
            disabled={busy}
            aria-label={t("acpControls.investigationInput")}
            placeholder={t("acpControls.investigationInput")}
            rows={2}
          />
          <Button type="submit" className="shrink-0" disabled={busy || !text.trim()}>
            {t("acpControls.submitInvestigation")}
          </Button>
        </div>
        {error && (
          <p role="alert" className="text-xs break-words text-rose-600 dark:text-rose-400">
            {error}
          </p>
        )}
      </form>
    </div>
  );
}
