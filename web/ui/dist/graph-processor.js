// AudioWorklet processor that owns the wasm patch Graph and renders audio on
// the audio thread. The React UI is the source of truth for graph structure;
// it posts mutations (addNode/removeNode/connect/disconnect/setParam/noteOn/
// noteOff) here, and we apply them to the mirrored Graph.
//
// AudioWorkletGlobalScope (Chromium) lacks `URL`, `fetch`, `TextDecoder`, and
// `TextEncoder`, so we polyfill both text codecs and receive the raw wasm bytes
// from the main thread (which can fetch), then compile + instantiate via the
// named `initSync` export.
import "./textdecoder-polyfill.js";
import { initSync, Graph } from "./pkg/web.js";

class GraphProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.graph = null;
    this.pending = [];
    this.port.onmessage = (event) => this.handleMessage(event.data);
  }

  async handleMessage(msg) {
    if (msg.op === "wasmBytes") {
      try {
        await initSync({ module: msg.bytes });
        this.graph = new Graph(sampleRate);
        for (const queued of this.pending) this.applyMutation(queued);
        this.pending = [];
        this.port.postMessage({ op: "ready" });
      } catch (err) {
        this.port.postMessage({ op: "error", message: String(err && err.stack || err) });
      }
      return;
    }
    if (this.graph) this.applyMutation(msg);
    else this.pending.push(msg);
  }

  applyMutation(msg) {
    try {
      switch (msg.op) {
        case "addNode":
          this.graph.add_node(msg.id, msg.kind);
          break;
        case "removeNode":
          this.graph.remove_node(msg.id);
          break;
        case "connect":
          this.graph.connect(msg.from, msg.fromPort, msg.to, msg.toPort);
          break;
        case "disconnect":
          this.graph.disconnect(msg.from, msg.fromPort, msg.to, msg.toPort);
          break;
        case "setParam":
          this.graph.set_param(msg.id, msg.name, msg.value);
          break;
        case "noteOn":
          this.graph.note_on(msg.id, msg.note, msg.vel ?? 127);
          break;
        case "noteOff":
          this.graph.note_off(msg.id);
          break;
      }
    } catch (err) {
      this.port.postMessage({ op: "error", message: String(err && err.stack || err) });
    }
  }

  process(_inputs, outputs) {
    const out = outputs[0];
    if (out.length > 0) {
      if (this.graph) {
        this.graph.process(out[0]);
      } else {
        out[0].fill(0);
      }
      for (let c = 1; c < out.length; c++) out[c].set(out[0]);
    }
    return true;
  }
}

registerProcessor("graph-processor", GraphProcessor);
