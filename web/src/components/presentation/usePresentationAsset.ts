import { useEffect, useRef, useState } from "react";

type Asset = { key: string; content: string | null; error: string; objectUrl?: string };

// Only fetched reading surfaces use this hook. Iframes keep their native lifecycle.
export function usePresentationAsset(
  href: string,
  resourceKey: string,
  kind: "markdown" | "image" | null,
) {
  const [asset, setAsset] = useState<Asset | null>(null);
  const refreshing = useRef(false);
  const key = `${kind}:${resourceKey}`;
  const current = kind && asset?.key === key ? asset : null;

  useEffect(() => {
    if (!kind || !href) {
      setAsset(null);
      return;
    }
    const controller = new AbortController();
    let pendingUrl: string | undefined;
    let definitiveFailure = false;
    refreshing.current = true;

    void (async () => {
      try {
        const response = await fetch(href, { cache: "no-store", signal: controller.signal });
        if (!response.ok) {
          // Denied, missing or invalid resources must not continue to look available.
          definitiveFailure =
            response.status >= 400 &&
            response.status < 500 &&
            response.status !== 408 &&
            response.status !== 429;
          throw new Error(`HTTP ${response.status}`);
        }
        let content: string;
        if (kind === "image") {
          pendingUrl = URL.createObjectURL(await response.blob());
          const image = new Image();
          image.src = pendingUrl;
          await image.decode();
          content = pendingUrl;
        } else {
          content = await response.text();
        }
        if (controller.signal.aborted) return;
        setAsset({ key, content, error: "", objectUrl: pendingUrl });
        // Ownership passes to the committed asset until replacement or unmount.
        pendingUrl = undefined;
      } catch (error) {
        if (controller.signal.aborted) return;
        const message = error instanceof Error ? error.message : String(error);
        setAsset((previous) =>
          !definitiveFailure && previous?.key === key
            ? { ...previous, error: message }
            : { key, content: null, error: message },
        );
      } finally {
        if (pendingUrl) URL.revokeObjectURL(pendingUrl);
        if (!controller.signal.aborted) refreshing.current = false;
      }
    })();
    return () => {
      controller.abort();
      refreshing.current = false;
    };
  }, [href, key, kind]);

  useEffect(() => {
    const url = asset?.objectUrl;
    return () => {
      if (url) URL.revokeObjectURL(url);
    };
  }, [asset?.objectUrl]);

  return {
    content: current?.content ?? null,
    error: current?.error ?? "",
    stale: current?.content != null && !!current.error,
    refreshing,
  };
}
