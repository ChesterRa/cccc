declare module "@novnc/novnc/lib/rfb" {
  export default class RFB extends EventTarget {
    viewOnly: boolean;
    scaleViewport: boolean;
    resizeSession: boolean;
    clipViewport: boolean;
    background: string;

    constructor(target: HTMLElement, url: string, options?: Record<string, unknown>);
    disconnect(): void;
  }
}
