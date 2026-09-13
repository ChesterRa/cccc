import { useEffect, useRef, useState, type ComponentProps } from "react";
import { useTranslation } from "react-i18next";
import { readFrameProof } from "../features/connect/protocol";
import { useUIStore } from "../stores/useUIStore";

// A navigation download leaves the embedded cookie partition. Fetching the
// authorized resource in the target document preserves its existing session.
export function AuthenticatedDownloadLink({
  href,
  download,
  onClick,
  children,
  ...props
}: ComponentProps<"a">) {
  const { t } = useTranslation("layout");
  const [pending, setPending] = useState(false);
  const request = useRef<AbortController | null>(null);
  useEffect(
    () => () => {
      request.current?.abort();
    },
    [],
  );
  return (
    <a
      {...props}
      href={href}
      download={download}
      aria-busy={pending || undefined}
      onClick={(event) => {
        onClick?.(event);
        if (event.defaultPrevented || !href || !readFrameProof(window.location)) return;
        const url = new URL(href, window.location.href);
        if (url.origin !== window.location.origin || !url.pathname.startsWith("/api/v1/groups/"))
          return;
        event.preventDefault();
        if (request.current) return;
        const controller = new AbortController();
        request.current = controller;
        setPending(true);
        void (async () => {
          const response = await fetch(url, {
            credentials: "same-origin",
            redirect: "error",
            signal: controller.signal,
          });
          if (!response.ok)
            throw new Error(t("connect.downloadFailed", { status: response.status }));
          const blob = await response.blob();
          if (controller.signal.aborted) return;
          const objectUrl = URL.createObjectURL(blob);
          const link = document.createElement("a");
          link.href = objectUrl;
          link.download = typeof download === "string" ? download : "download";
          document.body.appendChild(link);
          link.click();
          link.remove();
          window.setTimeout(() => URL.revokeObjectURL(objectUrl), 30000);
        })()
          .catch((error: unknown) => {
            if (!controller.signal.aborted)
              useUIStore
                .getState()
                .showError(error instanceof Error ? error.message : t("connectionFailed"));
          })
          .finally(() => {
            request.current = null;
            if (!controller.signal.aborted) setPending(false);
          });
      }}
    >
      {children}
      {pending ? (
        <span className="ml-1 text-xs" role="status">
          {t("connect.downloading")}
        </span>
      ) : null}
    </a>
  );
}
