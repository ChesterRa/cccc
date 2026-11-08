import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { DocumentPreview } from "../../messageBubble/DocumentPreview";
import { pauseOtherMessageMedia } from "../../messageBubble/messageMedia";
import { GraphicViewer } from "../../viewer/GraphicViewer";
import type { AttachmentResource } from "../../../stores/useResourcePanelStore";

export type AttachmentResourceMedia = AttachmentResource["media"];

type AttachmentResourceViewProps = {
  media: AttachmentResourceMedia;
  href: string;
  title: string;
  isSvg: boolean;
  isDark: boolean;
  /** Shown alone as the selected tab; only then does a video start by itself. */
  active: boolean;
  /** On screen, alone or on a side of the split view; by default, when active. */
  visible?: boolean;
  /** Stable across an optimistic upload being replaced by its stored URL. */
  resourceKey: string;
};

type SvgSource = { href: string; src: string; isSvg: boolean } | { href: string; error: true };

/** Own an SVG's image URL independently of its virtualized source message. */
function ResourceImage({ href, title, isSvg, resourceKey }: AttachmentResourceViewProps) {
  const { t } = useTranslation("chat");
  const [svg, setSvg] = useState<SvgSource | null>(null);

  useEffect(() => {
    if (!isSvg || !href) return undefined;
    const controller = new AbortController();
    let objectUrl = "";
    void (async () => {
      try {
        const response = await fetch(href, {
          credentials: "same-origin",
          signal: controller.signal,
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const blob = await response.blob();
        if (controller.signal.aborted) return;
        objectUrl = URL.createObjectURL(blob);
        setSvg({
          href,
          src: objectUrl,
          isSvg: blob.type.split(";", 1)[0].trim().toLowerCase() === "image/svg+xml",
        });
      } catch {
        if (!controller.signal.aborted) setSvg({ href, error: true });
      }
    })();
    return () => {
      controller.abort();
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [href, isSvg]);

  if (!href) {
    return (
      <p role="alert" className="p-4 text-sm">
        {t("imagePreviewUnavailable")}
      </p>
    );
  }
  if (isSvg) {
    if (!svg || svg.href !== href) {
      return (
        <p role="status" className="p-4 text-sm">
          {t("common:loading")}
        </p>
      );
    }
    if ("error" in svg) {
      return (
        <p role="alert" className="p-4 text-sm">
          {t("imagePreviewUnavailable")}
        </p>
      );
    }
    return <GraphicViewer src={svg.src} alt={title} isSvg={svg.isSvg} resourceKey={resourceKey} />;
  }
  return <GraphicViewer src={href} alt={title} resourceKey={resourceKey} />;
}

/** A hidden video keeps its playback position, but does not continue playing out of sight. */
function ResourceVideo({ href, title, active, visible = active }: AttachmentResourceViewProps) {
  const { t } = useTranslation("chat");
  const video = useRef<HTMLVideoElement>(null);
  const [failedHref, setFailedHref] = useState("");
  useEffect(() => {
    if (!visible) video.current?.pause();
  }, [visible]);
  if (failedHref === href) {
    return (
      <p role="alert" className="p-4 text-sm">
        {t("videoPlaybackUnavailable")}
      </p>
    );
  }
  return (
    <video
      ref={video}
      src={href}
      controls
      autoPlay={active}
      playsInline
      preload="metadata"
      data-message-media
      onPlay={pauseOtherMessageMedia}
      aria-label={title}
      className="h-full w-full bg-black object-contain"
      onError={() => setFailedHref(href)}
    />
  );
}

/** Renders an attachment without owning the surrounding panel or its tab state. */
export function AttachmentResourceView(props: AttachmentResourceViewProps) {
  const { t } = useTranslation("chat");
  if (props.media === "image") return <ResourceImage {...props} />;
  if (props.media === "video") return <ResourceVideo {...props} />;
  return (
    <DocumentPreview
      kind={props.media}
      href={props.href}
      title={props.title}
      isDark={props.isDark}
      failedText={t("documentPreviewFailed")}
    />
  );
}
