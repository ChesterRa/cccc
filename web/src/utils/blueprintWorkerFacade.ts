// Promise-based facade over the blueprint Web Worker
// Singleton pattern: shared across the app

import type { BlueprintBlock } from "../data/blueprints";

/** Raw block without order (input to worker) */
export interface RawBlock {
  x: number;
  y: number;
  z: number;
  color: string;
}

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

let _instance: BlueprintWorkerFacade | null = null;

export class BlueprintWorkerFacade {
  private worker: Worker;
  private pending = new Map<string, PendingRequest>();
  private nextId = 0;

  private constructor() {
    this.worker = new Worker(
      new URL("../workers/blueprint.worker.ts", import.meta.url),
      { type: "module" },
    );
    this.worker.onmessage = (e: MessageEvent) => {
      const { id, type, payload } = e.data;
      const req = this.pending.get(id);
      if (!req) return;
      this.pending.delete(id);
      if (type === "validationError" || type === "error") {
        req.reject(new Error(payload.error));
      } else {
        req.resolve(payload);
      }
    };
    this.worker.onerror = (err) => {
      for (const [id, req] of this.pending) {
        req.reject(new Error(`Worker error: ${err.message}`));
        this.pending.delete(id);
      }
    };
  }

  /** Get or create the singleton instance */
  static getInstance(): BlueprintWorkerFacade {
    if (!_instance) {
      _instance = new BlueprintWorkerFacade();
    }
    return _instance;
  }

  /** Validate raw blocks and compute build order */
  validate(
    blocks: RawBlock[],
    gridSize: [number, number, number],
  ): Promise<{ blocks: BlueprintBlock[]; gridSize: [number, number, number] }> {
    return this._post("validate", { blocks, gridSize });
  }

  /** Compute build order only (no validation) */
  computeOrder(blocks: RawBlock[]): Promise<{ blocks: BlueprintBlock[] }> {
    return this._post("computeOrder", { blocks });
  }

  /** Terminate the worker and release resources */
  dispose(): void {
    this.worker.terminate();
    for (const [, req] of this.pending) {
      req.reject(new Error("Worker disposed"));
    }
    this.pending.clear();
    if (_instance === this) _instance = null;
  }

  private _post<T>(type: string, payload: unknown): Promise<T> {
    return new Promise((resolve, reject) => {
      const id = String(this.nextId++);
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage({ id, type, payload });
    });
  }
}

// HMR cleanup: prevent stale Worker instances during development
if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    _instance?.dispose();
  });
}
