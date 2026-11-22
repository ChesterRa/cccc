import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { createSessionNodeInvitation } from "../../services/api/sessionNodes";
import { copyTextToClipboard } from "../../utils/copy";

type Ticket = { ticket: string; expires_at: string };
const valid = (ticket: Ticket) => Date.parse(ticket.expires_at) > Date.now();

function writePendingTicket(prepared: Promise<Ticket | null>, usable: (ticket: Ticket) => boolean) {
  if (
    typeof navigator === "undefined" ||
    typeof navigator.clipboard?.write !== "function" ||
    typeof ClipboardItem !== "function"
  )
    return null;
  const data = prepared.then((current) => {
    if (!current || !usable(current)) throw new Error("Pairing ticket unavailable");
    return new Blob([current.ticket], { type: "text/plain" });
  });
  // A constructor or write may reject before the request finishes consuming data.
  void data.catch(() => {});
  try {
    return navigator.clipboard.write([new ClipboardItem({ "text/plain": data })]).then(
      () => true,
      () => false,
    );
  } catch {
    return null;
  }
}

/** Tickets stay in memory; only explicit clicks create or copy them. */
export function useSessionNodeTicket(setError: (message: string) => void, onForbidden: () => void) {
  const { t } = useTranslation("layout");
  const [ticket, setTicket] = useState<Ticket | null>(null);
  const [ticketExpired, setTicketExpired] = useState(false);
  const [creating, setCreating] = useState(false);
  const [copying, setCopying] = useState(false);
  const [copied, setCopied] = useState(false);
  const alive = useRef(true);
  const pending = useRef(false);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    if (!ticket) return;
    const remaining = Date.parse(ticket.expires_at) - Date.now();
    setTicketExpired(!valid(ticket));
    if (!Number.isFinite(remaining) || remaining <= 0) return;
    const timer = setTimeout(
      () => {
        setTicketExpired(true);
        setCopied(false);
      },
      Math.min(remaining + 1, 2147483647),
    );
    return () => clearTimeout(timer);
  }, [ticket]);

  const copyTicket = async (current: Ticket) => {
    if (!alive.current || !valid(current)) return;
    setCopying(true);
    const success = await copyTextToClipboard(current.ticket);
    if (!alive.current) return;
    const usable = valid(current);
    setCopied(success && usable);
    setTicketExpired(!usable);
    if (!success && usable) setError(t("sessionNodes.copyFailed"));
  };

  const create = async () => {
    if (pending.current || !alive.current) return;
    pending.current = true;
    setCreating(true);
    setCopied(false);
    setError("");
    const prepared = (async (): Promise<Ticket | null> => {
      const response = await createSessionNodeInvitation();
      if (!alive.current) return null;
      if (!response.ok) {
        if (response.error.httpStatus === 401 || response.error.httpStatus === 403) onForbidden();
        else setError(t("sessionNodes.ticketFailed"));
        return null;
      }
      if (
        typeof response.result?.ticket !== "string" ||
        !response.result.ticket.trim() ||
        !Number.isFinite(Date.parse(response.result.expires_at))
      ) {
        setError(t("sessionNodes.ticketFailed"));
        return null;
      }
      setTicket(response.result);
      setTicketExpired(!valid(response.result));
      return response.result;
    })().catch(() => {
      if (alive.current) setError(t("sessionNodes.ticketFailed"));
      return null;
    });
    // Reserve the clipboard during this click; only its bytes wait for the hub.
    const writing = writePendingTicket(prepared, (current) => alive.current && valid(current));
    try {
      const current = await prepared;
      if (!current || !alive.current || !valid(current)) return;
      setCreating(false);
      setCopying(true);
      const success = writing ? await writing : false;
      if (!alive.current || !valid(current)) return;
      if (success) setCopied(true);
      else await copyTicket(current);
    } finally {
      pending.current = false;
      if (alive.current) {
        setCreating(false);
        setCopying(false);
      }
    }
  };

  const copy = async () => {
    if (pending.current || !alive.current) return;
    // Also check the clock: background-tab timer throttling can leave the view
    // behind the actual expiry. This click still generates and copies once.
    if (!ticket || !valid(ticket)) return create();
    pending.current = true;
    setCopied(false);
    setError("");
    try {
      await copyTicket(ticket);
    } finally {
      pending.current = false;
      if (alive.current) setCopying(false);
    }
  };

  return { ticket, ticketExpired, creating, copying, copied, create, copy };
}
