// Audio engine singleton: owns the AudioContext + AudioWorkletNode that runs
// the wasm Graph. The React UI calls these methods to mirror its node/edge
// state into the worklet. The worklet is the audio-rate processor; this layer
// just ferries mutations and the initial wasm bytes across the thread boundary.

import type { KindCode } from "./nodeSpec";

type WorkletNode = AudioWorkletNode;

class AudioEngine {
  private ctx: AudioContext | null = null;
  private node: WorkletNode | null = null;
  private ready = false;
  private readyPromise: Promise<void> | null = null;

  get isReady() {
    return this.ready;
  }

  async init(): Promise<void> {
    if (this.readyPromise) return this.readyPromise;
    this.readyPromise = this.doInit();
    return this.readyPromise;
  }

  private async doInit(): Promise<void> {
    const ctx = new AudioContext({ sampleRate: 48000 });
    this.ctx = ctx;

    if (!ctx.audioWorklet) {
      throw new Error(
        "AudioWorklet unavailable: open the app via http://localhost (secure context), not file:// or a LAN IP."
      );
    }

    const master = ctx.createGain();
    master.gain.value = 0.4;
    master.connect(ctx.destination);

    await ctx.audioWorklet.addModule("graph-processor.js", { type: "module" } as WorkletOptions);

    const node = new AudioWorkletNode(ctx, "graph-processor", {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [2],
    });
    node.connect(master);
    this.node = node;

    node.port.onmessage = (event) => {
      if (event.data?.op === "error") console.error("[worklet]", event.data.message);
    };

    const wasmBytes = await (await fetch("pkg/web_bg.wasm")).arrayBuffer();
    await new Promise<void>((resolve, reject) => {
      const handler = (event: MessageEvent) => {
        if (event.data?.op === "ready") {
          node.port.removeEventListener("message", handler);
          resolve();
        } else if (event.data?.op === "error") {
          node.port.removeEventListener("message", handler);
          reject(new Error(event.data.message));
        }
      };
      node.port.addEventListener("message", handler);
      node.port.postMessage({ op: "wasmBytes", bytes: wasmBytes }, [wasmBytes]);
    });

    this.ready = true;
  }

  async resume() {
    if (this.ctx && this.ctx.state !== "running") {
      await this.ctx.resume();
    }
  }

  private post(msg: unknown) {
    if (!this.node) return;
    this.node.port.postMessage(msg);
  }

  addNode(id: number, kind: KindCode) {
    this.post({ op: "addNode", id, kind });
  }

  removeNode(id: number) {
    this.post({ op: "removeNode", id });
  }

  connect(from: number, fromPort: number, to: number, toPort: number) {
    this.post({ op: "connect", from, fromPort, to, toPort });
  }

  disconnect(from: number, fromPort: number, to: number, toPort: number) {
    this.post({ op: "disconnect", from, fromPort, to, toPort });
  }

  setParam(id: number, name: string, value: number) {
    this.post({ op: "setParam", id, name, value });
  }

  noteOn(id: number, note: number, vel = 127) {
    this.post({ op: "noteOn", id, note, vel });
  }

  noteOff(id: number) {
    this.post({ op: "noteOff", id });
  }
}

export const audioEngine = new AudioEngine();
