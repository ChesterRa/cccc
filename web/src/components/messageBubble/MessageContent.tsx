import { useState, type KeyboardEvent, type MouseEvent } from "react";
import { classNames } from "../../utils/classNames";
import type { MessageMentionToken } from "../../utils/mentionHighlight";
import {
  messageLinkPreview,
  type MessageLinkPreview as Preview,
} from "../../utils/messageLinkPreview";
import { messageHrefPath } from "../../utils/messagePaths";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { MessageLinkPreview } from "./MessageLinkPreview";
import { MessageMentionText } from "./MessageMentionText";
import { isModifiedClick } from "./messageMedia";
import type { MessagePathTarget } from "./messagePathContext";

function PlainMessageText({
  text,
  className,
  mentionTokens,
  pathRoot,
}: {
  text: string;
  className?: string;
  mentionTokens: readonly MessageMentionToken[];
  pathRoot?: string;
}) {
  return (
    <div
      className={classNames(
        "break-words whitespace-pre-wrap text-[var(--color-text-primary)] [overflow-wrap:anywhere]",
        className,
      )}
    >
      <MessageMentionText text={text} mentionTokens={mentionTokens} pathRoot={pathRoot} />
    </div>
  );
}

export function MessageContent(props: {
  fallbackText: string;
  shouldRenderMarkdown: boolean;
  isDark: boolean;
  mentionTokens: readonly MessageMentionToken[];
  /** Given when this message's file paths can open. */
  paths?: MessagePathTarget | null;
}) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const pathAt = (target: EventTarget) =>
    (target as Element | null)?.closest("[data-message-path]")?.getAttribute("data-message-path");
  const openPathKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const path = event.key === "Enter" || event.key === " " ? pathAt(event.target) : null;
    if (!path) return;
    event.preventDefault();
    props.paths?.open(path);
  };
  const openLinkPreview = (event: MouseEvent<HTMLDivElement>) => {
    const path = pathAt(event.target);
    if (path) {
      props.paths?.open(path);
      return;
    }
    if (isModifiedClick(event)) return;
    const href = (event.target as Element | null)?.closest("a[href]")?.getAttribute("href") || "";
    if (!href) return;
    // Left to the browser, a link to a project file would replace CCCC.
    const linkedPath = props.paths && messageHrefPath(href, props.paths.rootPath);
    if (linkedPath) {
      event.preventDefault();
      props.paths?.open(linkedPath);
      return;
    }
    const next = messageLinkPreview(href);
    if (!next) return;
    event.preventDefault();
    setPreview(next);
  };
  const plainText = (
    <PlainMessageText
      text={props.fallbackText}
      className="max-w-full"
      mentionTokens={props.mentionTokens}
      pathRoot={props.paths?.rootPath}
    />
  );
  return (
    <>
      {/* Layout stays the content's own; this element only hears clicks on its links. */}
      <div
        className="contents"
        data-message-content
        onClick={openLinkPreview}
        onKeyDown={openPathKey}
      >
        {props.shouldRenderMarkdown ? (
          <LazyMarkdownRenderer
            content={props.fallbackText}
            isDark={props.isDark}
            invertText={false}
            enableMermaid
            className="max-w-full break-words text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
            mentionTokens={props.mentionTokens}
            pathRoot={props.paths?.rootPath}
            fallback={plainText}
          />
        ) : (
          plainText
        )}
      </div>
      {/* Outside that element: clicks in a portal still bubble to its React parent. */}
      {preview ? (
        <MessageLinkPreview
          preview={preview}
          isDark={props.isDark}
          onClose={() => setPreview(null)}
        />
      ) : null}
    </>
  );
}
