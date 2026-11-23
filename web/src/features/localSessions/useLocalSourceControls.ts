import { useEffect, useRef, useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useGroupStore } from "../../stores";
import type { LocalSessionSource } from "../../types";
import { controlLocalSource, recheckLocalSource } from "../../services/api/localSessionControls";
import type { useLocalSessionSources } from "./useLocalSessionSources";

export function useLocalSourceControls(
  directory: ReturnType<typeof useLocalSessionSources>,
  groupBusy: RefObject<boolean>,
) {
  const { t } = useTranslation("layout");
  const [target, setTarget] = useState<{
    source: LocalSessionSource;
    action: "remove" | "restore";
  } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [cooldowns, setCooldowns] = useState<Record<string, number>>({});
  const pending = useRef(false);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    const deadlines = Object.values(cooldowns);
    if (!deadlines.length) return;
    const timer = setTimeout(
      () =>
        setCooldowns((previous) =>
          Object.fromEntries(Object.entries(previous).filter(([, at]) => at > Date.now())),
        ),
      Math.max(0, Math.min(...deadlines) - Date.now()) + 1,
    );
    return () => clearTimeout(timer);
  }, [cooldowns]);
  const open = (source: LocalSessionSource, action: "remove" | "restore") => {
    if (pending.current || groupBusy.current) return;
    setError("");
    setNotice("");
    setTarget({ source, action });
  };
  const confirm = async () => {
    if (!target || pending.current || groupBusy.current) return;
    const control = directory.source_states?.[target.source.source_id]?.control;
    if (!directory.capabilities?.source_control || !control?.supported) return;
    pending.current = true;
    setBusy(true);
    setError("");
    directory.invalidate();
    try {
      const response = await controlLocalSource(
        target.source.source_id,
        target.action,
        control.management_revision,
      );
      if (!alive.current) return;
      if (response.ok) {
        directory.apply(target.source, response.result);
        const groups = useGroupStore.getState();
        void Promise.allSettled([
          groups.refreshGroups(),
          ...(groups.selectedGroupId ? [groups.refreshActors(groups.selectedGroupId)] : []),
        ]);
        const reached =
          target.action === "remove"
            ? response.result.access_blocked
            : !response.result.access_blocked;
        if (!reached || response.result.status === "partial" || !response.result.pause_complete) {
          setNotice(
            t(
              response.result.access_blocked
                ? "sourceControls.partial"
                : "sourceControls.incomplete",
            ),
          );
        } else {
          setNotice(
            t(
              response.result.access_blocked
                ? "sourceControls.removedNotice"
                : "sourceControls.restoredNotice",
            ),
          );
          setTarget(null);
        }
      } else
        setError(
          t(
            response.error.code === "source_control_conflict"
              ? "sourceControls.conflict"
              : "sourceControls.unconfirmed",
          ),
        );
      await directory.reload();
    } catch {
      if (alive.current) {
        setError(t("sourceControls.unconfirmed"));
        await directory.reload();
      }
    } finally {
      pending.current = false;
      if (alive.current) setBusy(false);
    }
  };
  const recheck = async (source: LocalSessionSource) => {
    if (pending.current || groupBusy.current || cooldowns[source.source_id] > Date.now()) return;
    if (
      !directory.capabilities?.source_diagnostics ||
      !directory.source_states?.[source.source_id]?.control.supported
    )
      return;
    pending.current = true;
    setBusy(true);
    setError("");
    directory.invalidate();
    try {
      const response = await recheckLocalSource(source.source_id);
      if (!alive.current) return;
      if (response.ok) {
        directory.diagnose(source.source_id, response.result.diagnostic);
        const delay = Math.max(0, response.result.retry_after_ms);
        if (delay)
          setCooldowns((previous) => ({ ...previous, [source.source_id]: Date.now() + delay }));
        setNotice(
          t(
            response.result.check_status === "busy"
              ? "sourceControls.checkBusy"
              : response.result.check_status === "cached"
                ? "sourceControls.checkCached"
                : "sourceControls.checked",
          ),
        );
      } else setError(t("sourceControls.checkFailed"));
      await directory.reload();
    } catch {
      if (alive.current) setError(t("sourceControls.checkFailed"));
    } finally {
      pending.current = false;
      if (alive.current) setBusy(false);
    }
  };
  return {
    targetState: target ? directory.source_states?.[target.source.source_id] : undefined,
    target,
    busy,
    pending,
    error,
    notice,
    cooldowns,
    open,
    confirm,
    recheck,
    close: () => {
      if (!pending.current) {
        setTarget(null);
        setError("");
      }
    },
  };
}
