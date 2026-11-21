import { useEffect, useRef, useState } from "react";
import {
  decideSessionNodeJoin,
  fetchSessionNodeJoin,
  type SessionNodeJoinDetail,
} from "../../services/api/sessionNodes";

type View =
  | { kind: "loading" | "missing" | "forbidden" | "failed" | "expired" }
  | { kind: "request"; request: SessionNodeJoinDetail };

function problem(status: number | undefined): View {
  if (status === 401 || status === 403) return { kind: "forbidden" };
  if (status === 404) return { kind: "missing" };
  if (status === 410) return { kind: "expired" };
  return { kind: "failed" };
}

function verified(id: string, request: SessionNodeJoinDetail): boolean {
  return (
    request?.request_id === id &&
    typeof request.name === "string" &&
    !!request.name.trim() &&
    ["pending", "approved", "denied", "expired"].includes(request.status) &&
    (request.status !== "approved" ||
      (typeof request.node_id === "string" && !!request.node_id.trim())) &&
    (request.status !== "pending" ||
      (typeof request.user_code === "string" &&
        !!request.user_code.trim() &&
        typeof request.node_id === "string" &&
        !!request.node_id.trim() &&
        Number.isFinite(Date.parse(request.expires_at))))
  );
}

export function useSessionNodeJoinReview(id: string) {
  const [view, setView] = useState<View>({ kind: "loading" });
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [revision, setRevision] = useState(0);
  const sequence = useRef<symbol | null>(null);
  const deciding = useRef(false);
  const refresh = () => setRevision((value) => value + 1);

  useEffect(() => {
    const generation = Symbol();
    sequence.current = generation;
    deciding.current = false;
    setBusy(false);
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function read() {
      if (generation !== sequence.current) return;
      if (!/^[a-zA-Z0-9_-]{1,128}$/.test(id)) {
        setView({ kind: "missing" });
        return;
      }
      try {
        const response = await fetchSessionNodeJoin(id, controller.signal);
        if (generation !== sequence.current) return;
        if (!response.ok) {
          setView(problem(response.error.httpStatus));
          return;
        }
        const request = response.result.request;
        if (!verified(id, request)) {
          setView({ kind: "failed" });
          return;
        }
        if (request.status === "pending") {
          const remaining = Date.parse(request.expires_at) - Date.now();
          // Only the hub can expire a request. The browser clock just schedules its next read.
          timer = setTimeout(() => void read(), remaining > 0 ? Math.min(5000, remaining) : 5000);
        }
        setView({ kind: "request", request });
      } catch {
        if (generation === sequence.current) setView({ kind: "failed" });
      }
    }
    setView({ kind: "loading" });
    void read();
    return () => {
      sequence.current = null;
      controller.abort();
      clearTimeout(timer);
    };
  }, [id, revision]);

  async function decide(decision: "approve" | "deny") {
    if (deciding.current || view.kind !== "request" || view.request.status !== "pending") return;
    const { request } = view;
    deciding.current = true;
    const generation = Symbol();
    sequence.current = generation;
    setBusy(true);
    setUncertain(false);
    try {
      const result = await decideSessionNodeJoin(request.request_id, decision);
      if (generation !== sequence.current) return;
      const expected = decision === "approve" ? "approved" : "denied";
      if (result.ok && result.result?.status === expected) {
        setView({ kind: "request", request: { ...request, status: expected } });
      } else if (
        !result.ok &&
        (result.error.httpStatus === 401 || result.error.httpStatus === 403)
      ) {
        setView({ kind: "forbidden" });
      } else {
        setUncertain(true);
        refresh();
      }
    } catch {
      if (generation === sequence.current) {
        setUncertain(true);
        refresh();
      }
    } finally {
      if (generation === sequence.current) {
        deciding.current = false;
        setBusy(false);
      }
    }
  }
  return { view, busy, uncertain, decide, refresh };
}
