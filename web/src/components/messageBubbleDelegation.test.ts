import { describe, expect, it } from "vitest";

import {
  destinationChipKey,
  getDelegationDisplayText,
  getDelegationProtocolText,
  getDelegationSourceOutboundStatus,
  isDelegationSourceOutboundEvent,
  isDelegationRequestText,
  isDelegationSourceOutbound,
  isDelegationResultText,
} from "./messageBubbleDelegation";

const RAW_REQUEST = [
  "你好，我是来自「g_src」的 agent-a。用户让我来联系你。",
  "",
  "打个招呼，问一问看看最近别人问得最多的问题是什么。",
  "",
  "方便的话，请直接回复我这边。",
  "",
  "<!-- cccc-delegation-protocol",
  "[cccc-delegation:v1]",
  "delegation_id: dlg_1",
  "source_group_id: g_src",
  "target_group_id: g_dst",
  "target_actor_id: target",
  "source_contact: send back with cccc_message_send(dst_group_id=g_src, ...)",
  "target_contact: reply in this group first",
  "",
  "Communication protocol:",
  "Do not treat #tokens in the user message as recipients in your group.",
  "",
  "Original user message (reference only):",
  "总结两个 skill,跟 #self-agent 说一下",
  "[/cccc-delegation]",
  "-->",
].join("\n");

describe("delegation natural body / protocol split", () => {
  it("display text drops the protocol comment and keeps only the natural contact body", () => {
    const shown = getDelegationDisplayText(RAW_REQUEST);
    expect(shown).toContain("你好");
    expect(shown).toContain("用户让我来联系你");
    expect(shown).toContain("最近别人问得最多的问题是什么");
    expect(shown).not.toContain("总结两个 skill");
    expect(shown).not.toContain("#self-agent");
    expect(shown).not.toContain("自然任务");
    expect(shown).not.toContain("不要把用户原话");
    expect(shown).not.toContain("请先确认是否接收");
    expect(shown).not.toContain("[cccc-delegation:v1]");
    expect(shown).not.toContain("delegation_id:");
    expect(shown).not.toContain("source_contact:");
    expect(shown).not.toContain("cccc-delegation-protocol");
  });

  it("protocol text extracts the full machine block", () => {
    const protocol = getDelegationProtocolText(RAW_REQUEST);
    expect(protocol).toContain("[cccc-delegation:v1]");
    expect(protocol).toContain("delegation_id: dlg_1");
    expect(protocol).toContain("source_contact:");
    expect(protocol).toContain("Original user message (reference only):");
    expect(protocol).toContain("总结两个 skill");
  });

  it("raw text still classifies as a delegation request → relayedTo chip", () => {
    expect(isDelegationRequestText(RAW_REQUEST)).toBe(true);
    expect(destinationChipKey(RAW_REQUEST)).toBe("relayedTo");
  });

  it("source-side outbound delegation is a status, not visible relay prose", () => {
    expect(isDelegationSourceOutbound({
      rawText: RAW_REQUEST,
      dstGroupId: "g_dst",
    })).toBe(true);
    expect(isDelegationSourceOutbound({
      rawText: RAW_REQUEST,
      srcGroupId: "g_src",
    })).toBe(false);
    const status = getDelegationSourceOutboundStatus(RAW_REQUEST);
    expect(status).toContain("已联系目标组");
    expect(status).not.toContain("用户让我来联系你");
    expect(status).not.toContain("方便的话，请直接回复我这边");
    expect(status).not.toContain("[cccc-delegation:v1]");
  });

  it("classifies source outbound relay audit events separately from target inbound delegation", () => {
    expect(isDelegationSourceOutboundEvent({
      text: RAW_REQUEST,
      dst_group_id: "g_dst",
      dst_to: ["target"],
    })).toBe(true);
    expect(isDelegationSourceOutboundEvent({
      text: RAW_REQUEST,
      src_group_id: "g_src",
      src_event_id: "ev_src",
      to: ["target"],
    })).toBe(false);
  });

  it("a plain message is unchanged by getDelegationDisplayText", () => {
    expect(getDelegationDisplayText("just a normal message")).toBe("just a normal message");
    expect(getDelegationProtocolText("just a normal message")).toBe("");
  });
});

describe("MessageBubble delegation display wiring", () => {
  it("uses source-outbound status before falling back to natural display text", async () => {
    const { readFileSync } = await import("node:fs");
    const { dirname, join } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "MessageBubble.tsx"), "utf8");
    expect(source).toContain("isDelegationSourceOutbound({");
    expect(source).toContain("getDelegationSourceOutboundStatus(displayMessageText)");
    expect(source.indexOf("getDelegationSourceOutboundStatus(displayMessageText)")).toBeLessThan(
      source.indexOf("getDelegationDisplayText(displayMessageText)"),
    );
  });

  it("renders Group Bridge source as metadata instead of an open-original action", async () => {
    const { readFileSync } = await import("node:fs");
    const { dirname, join } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "MessageBubble.tsx"), "utf8");
    expect(source).toContain('remoteBadgeLabel={remoteBadgeLabel || undefined}');
    expect(source).toContain('const isGroupBridgeSource = sourcePlatform === "group_bridge_session" || String(ev.by || "").startsWith("group_bridge:");');
    expect(source).toContain('hasSource && !isGroupBridgeSource');
    expect(source).toContain('t("remoteBadge"');
    expect(source).toContain('t("relayedFrom", { label: sourceLabel })');
    expect(source).not.toContain("openOriginalMessage");
    expect(source).toContain("onOpenSource?.(srcGroupId, srcEventId)");
    expect(source).not.toContain("remoteSourceDetails");
    expect(source).not.toContain('t("relayedFrom", { groupId: srcGroupId');
  });

  it("keeps local relay source chips clickable while remote sources stay non-local", async () => {
    const { readFileSync } = await import("node:fs");
    const { dirname, join } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const bubbleSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "MessageBubble.tsx"), "utf8");
    const listSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "VirtualMessageList.tsx"), "utf8");
    const chatTabSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../pages/chat/ChatTab.tsx"), "utf8");
    const tabSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../hooks/useChatTab.ts"), "utf8");

    expect(bubbleSource).toContain('onClick={() => onOpenSource?.(srcGroupId, srcEventId)}');
    expect(bubbleSource).toContain('disabled={!onOpenSource}');
    expect(listSource).toContain('onOpenSource={onOpenSource}');
    expect(chatTabSource).toContain('onOpenSource={openSourceMessage}');
    expect(tabSource).toContain("function canOpenSourceMessageLocally");
    expect(tabSource).toContain("return !group.group_bridge_remote");
    expect(tabSource).toContain("openSourceMessage");
  });

  it("conversation list filters source outbound delegation audit events", async () => {
    const { readFileSync } = await import("node:fs");
    const { dirname, join } = await import("node:path");
    const { fileURLToPath } = await import("node:url");
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../hooks/useChatTab.ts"), "utf8");
    expect(source).toContain("shouldShowInConversation");
    expect(source).toContain("filter(shouldShowInConversation)");
    expect(source).toContain("isDelegationSourceOutboundEvent(event.data)");
  });
});

describe("messageBubbleDelegation", () => {
  it("classifies a delegation request message", () => {
    const text = "[cccc-delegation:v1]\ndelegation_id: dlg_1\nOriginal request:\ndo x\n[/cccc-delegation]";
    expect(isDelegationRequestText(text)).toBe(true);
    expect(destinationChipKey(text)).toBe("relayedTo");
  });

  it("classifies a delegation result message", () => {
    const text = "[cccc-delegation-result:v1]\ndelegation_id: dlg_1\nstatus: done\n[/cccc-delegation-result]";
    expect(isDelegationResultText(text)).toBe(true);
    expect(isDelegationRequestText(text)).toBe(false);
  });

  it("a plain cross-group message keeps the Sent to chip", () => {
    expect(isDelegationRequestText("hello there")).toBe(false);
    expect(destinationChipKey("a normal forwarded note")).toBe("sentTo");
  });
});
