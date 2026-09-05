// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { DndContext, MouseSensor, useDraggable, useSensor, useSensors } from "@dnd-kit/core";
import { expect, it, vi } from "vite-plus/test";
import { GroupItemMenuTrigger } from "./GroupItemMenuTrigger";

it("isolates menu mouse gestures while the row remains draggable", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const onDragStart = vi.fn();
  const onToggle = vi.fn();
  function Row() {
    const { setNodeRef, listeners, attributes } = useDraggable({ id: "group" });
    return (
      <div ref={setNodeRef} {...listeners} {...attributes} data-row>
        Group
        <GroupItemMenuTrigger isActive label="Actions" open={false} onToggle={onToggle} />
      </div>
    );
  }
  function Fixture() {
    const sensors = useSensors(useSensor(MouseSensor, { activationConstraint: { distance: 4 } }));
    return (
      <DndContext sensors={sensors} onDragStart={onDragStart}>
        <Row />
      </DndContext>
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const mouse = (target: Element | Document, type: string, x: number) =>
    act(async () => {
      target.dispatchEvent(
        new MouseEvent(type, {
          bubbles: true,
          cancelable: true,
          button: 0,
          buttons: type === "mouseup" ? 0 : 1,
          clientX: x,
          clientY: 10,
        }),
      );
    });
  try {
    await act(async () => root.render(<Fixture />));
    const trigger = host.querySelector("button")!;
    await mouse(trigger, "mousedown", 10);
    await mouse(document, "mousemove", 30);
    expect(onDragStart).not.toHaveBeenCalled();
    await mouse(document, "mouseup", 30);
    await act(async () => trigger.click());
    expect(onToggle).toHaveBeenCalledOnce();
    await mouse(host.querySelector("[data-row]")!, "mousedown", 10);
    await mouse(document, "mousemove", 30);
    expect(onDragStart).toHaveBeenCalledOnce();
    await mouse(document, "mouseup", 30);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
