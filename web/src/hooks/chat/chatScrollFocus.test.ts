import { describe, expect, it } from "vite-plus/test";
import type { ChatScrollSnapshot } from "../../stores/useUIStore";
import type { LedgerEvent } from "../../types";
import { pickScrollFocusTarget, resolveInitialChatScroll } from "./chatScrollFocus";

const message = (id: string, extra: Record<string, unknown> = {}): LedgerEvent => ({
  id,
  kind: "chat.message",
  by: "agent",
  data: { text: id, ...extra },
});
const conversation = ["a", "b", "c", "d", "e", "f", "g"].map((id) => message(id));
const pick = (focusId: string, view: string[], conv = conversation) =>
  pickScrollFocusTarget({ focusId, conversation: conv, view: view.map((id) => message(id)) });

describe("pickScrollFocusTarget", () => {
  it("keeps the message being read when the new view has it", () => {
    expect(pick("d", ["b", "d", "f"])).toBe("d");
  });

  it("takes the nearest neighbour in conversation order, the older one on a tie", () => {
    expect(pick("d", ["b", "f"])).toBe("b");
    expect(pick("d", ["a", "e"])).toBe("e");
    expect(pick("a", ["g"])).toBe("g");
  });

  it("finds neighbours for disjoint filters through the whole conversation", () => {
    // The focus came from one filter (say Requests) and the new view (say Mail) shares
    // nothing with it, but both sit in the loaded conversation.
    expect(pick("c", ["f", "g"])).toBe("f");
  });

  it("never picks an optimistic row", () => {
    const conv = [message("a"), message("b", { _optimistic: true }), message("c")];
    const view = [message("b", { _optimistic: true }), message("c")];
    expect(pickScrollFocusTarget({ focusId: "b", conversation: conv, view })).toBe("");
    expect(pickScrollFocusTarget({ focusId: "a", conversation: conv, view })).toBe("c");
  });

  it("returns nothing when the message is not loaded or the view shares nothing", () => {
    expect(pick("z", ["b", "f"])).toBe("");
    expect(pick("d", ["x", "y"])).toBe("");
    expect(pick("", ["d"])).toBe("");
  });
});

describe("resolveInitialChatScroll", () => {
  // Read in All: the top-edge row is b, the message being read is d, 40px above the top.
  const saved: ChatScrollSnapshot = {
    coordinateVersion: 1,
    mode: "detached",
    anchorId: "b",
    offsetPx: 12,
    scrollTop: 900,
    updatedAt: 1,
    viewKey: "g:live:all:[]",
    focusId: "d",
    focusOffsetPx: 40,
  };
  const open = (
    viewKey: string,
    view: string[],
    snapshot: ChatScrollSnapshot | null = saved,
    exactOnly = false,
  ) =>
    resolveInitialChatScroll({
      snapshot,
      viewKey,
      conversation,
      view: view.map((id) => message(id)),
      exactOnly,
    });

  it("brings back a place saved in this view exactly, older shapes included", () => {
    const exact = { anchorId: "b", anchorOffsetPx: 12, scrollOffsetPx: 900 };
    expect(open("g:live:all:[]", ["a", "b", "d"])).toEqual(exact);
    expect(open("g:live:mail:[]", ["a"], { ...saved, viewKey: undefined })).toEqual(exact);
  });

  it("keeps the message being read at its height when another filter has it", () => {
    expect(open("g:live:user:[]", ["c", "d"])).toEqual({ anchorId: "d", anchorOffsetPx: 40 });
  });

  it("opens a marked neighbour at its top, since a height inside one message fits no other", () => {
    expect(open("g:live:mail:[]", ["c", "f"])).toEqual({ anchorId: "c", highlight: true });
  });

  it("keeps only the message itself in an exact-only view", () => {
    expect(open("g:live:pending_user:[]", ["c", "f"], saved, true)).toEqual({});
    expect(open("g:live:pending_user:[]", ["d", "f"], saved, true)).toEqual({
      anchorId: "d",
      anchorOffsetPx: 40,
    });
  });

  describe("with the messages that were on screen", () => {
    // On screen in All: d across the middle, being read, 40px above the top; e and f below it.
    const onScreen: ChatScrollSnapshot = {
      ...saved,
      visible: [
        { id: "d", offsetPx: 40 },
        { id: "e", offsetPx: -160 },
        { id: "f", offsetPx: -240 },
      ],
    };

    it("keeps the first one on screen that the new view shows, at its height", () => {
      // The message being read is not in User-related, but e was on screen and is.
      expect(open("g:live:user:[]", ["a", "c", "e"], onScreen)).toEqual({
        anchorId: "e",
        anchorOffsetPx: -160,
      });
      expect(open("g:live:user:[]", ["c", "d"], onScreen)).toEqual({
        anchorId: "d",
        anchorOffsetPx: 40,
      });
    });

    it("prefers a message on screen to a nearer one off it, even in an exact-only view", () => {
      // c is d's nearest neighbour in conversation order, but only f was on screen.
      expect(open("g:live:mail:[]", ["c", "f"], onScreen)).toEqual({
        anchorId: "f",
        anchorOffsetPx: -240,
      });
      expect(open("g:live:pending_user:[]", ["c", "f"], onScreen, true)).toEqual({
        anchorId: "f",
        anchorOffsetPx: -240,
      });
    });

    it("falls back to a marked neighbour only when nothing on screen is shown", () => {
      expect(open("g:live:mail:[]", ["a", "g"], onScreen)).toEqual({
        anchorId: "a",
        highlight: true,
      });
    });
  });

  describe("from a place at the bottom", () => {
    const atBottom: ChatScrollSnapshot = {
      coordinateVersion: 1,
      mode: "follow",
      anchorId: "",
      offsetPx: 0,
      updatedAt: 1,
      viewKey: "g:live:mail:[]",
    };

    it("opens at the bottom of the same view, or of another without a touched message", () => {
      expect(open("g:live:mail:[]", ["c", "f"], atBottom)).toEqual({});
      expect(
        open(
          "g:live:all:[]",
          conversation.map((m) => String(m.id)),
          atBottom,
        ),
      ).toEqual({});
    });

    it("hands on a touched message, the only place a list too short to scroll can give", () => {
      const touched = {
        ...atBottom,
        focusId: "c",
        focusOffsetPx: -120,
        visible: [{ id: "c", offsetPx: -120 }],
      };
      expect(
        open(
          "g:live:all:[]",
          conversation.map((m) => String(m.id)),
          touched,
        ),
      ).toEqual({ anchorId: "c", anchorOffsetPx: -120 });
      // Not shown there: no neighbour stands in for it; the view opens at its own bottom.
      expect(open("g:live:user:[]", ["a", "g"], touched)).toEqual({});
      // The same view keeps following the latest messages.
      expect(open("g:live:mail:[]", ["c", "f"], touched)).toEqual({});
    });
  });

  it("uses the top-edge row when no message being read was saved", () => {
    const plain = { ...saved, focusId: undefined, focusOffsetPx: undefined };
    expect(open("g:live:user:[]", ["b"], plain)).toEqual({ anchorId: "b", anchorOffsetPx: 12 });
  });

  it("starts at the view's own default without a place to restore", () => {
    expect(open("g:live:all:[]", ["a"], null)).toEqual({});
  });
});
