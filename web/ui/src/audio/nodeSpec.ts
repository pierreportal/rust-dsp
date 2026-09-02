// Mirror of the Rust `Kind` enum + port layout in web/src/graph.rs.
// Kind codes MUST stay in sync with the wasm Graph.add_node codes.

export const Kind = {
  Osc: 0,
  Adsr: 1,
  Filter: 2,
  Distortion: 3,
  Vca: 4,
  Mixer: 5,
  Constant: 7,
  Out: 8,
  Midi: 9,
} as const;

export type KindCode = (typeof Kind)[keyof typeof Kind];

export interface ParamSpec {
  name: string;
  label: string;
  min: number;
  max: number;
  step: number;
  default: number;
}

export interface NodeSpec {
  kind: KindCode;
  label: string;
  color: string;
  inputs: string[];
  outputs: string[];
  params: ParamSpec[];
}

const t = (min: number, max: number, step: number, def: number) => ({ min, max, step, default: def });

export const NODE_SPECS: Record<KindCode, NodeSpec> = {
  [Kind.Osc]: {
    kind: Kind.Osc,
    label: "Oscillator",
    color: "#3b82f6",
    inputs: ["freq cv"],
    outputs: ["signal"],
    params: [
      { name: "freq", label: "freq (Hz)", ...t(20, 4000, 1, 220) },
      { name: "waveform", label: "wave (0-4)", ...t(0, 4, 1, 1) },
      { name: "pulseWidth", label: "pulse width", ...t(0.05, 0.95, 0.01, 0.5) },
    ],
  },
  [Kind.Adsr]: {
    kind: Kind.Adsr,
    label: "ADSR",
    color: "#a855f7",
    inputs: ["gate"],
    outputs: ["env"],
    params: [
      { name: "attack", label: "attack (s)", ...t(0.001, 2, 0.001, 0.01) },
      { name: "decay", label: "decay (s)", ...t(0.001, 2, 0.001, 0.15) },
      { name: "sustain", label: "sustain", ...t(0, 1, 0.01, 0.7) },
      { name: "release", label: "release (s)", ...t(0.001, 3, 0.001, 0.3) },
    ],
  },
  [Kind.Filter]: {
    kind: Kind.Filter,
    label: "Filter (SVF)",
    color: "#06b6d4",
    inputs: ["signal", "cutoff cv"],
    outputs: ["signal"],
    params: [
      { name: "cutoff", label: "cutoff (Hz)", ...t(40, 12000, 1, 1200) },
      { name: "resonance", label: "resonance", ...t(0.05, 1, 0.01, 0.2) },
    ],
  },
  [Kind.Distortion]: {
    kind: Kind.Distortion,
    label: "Distortion",
    color: "#f97316",
    inputs: ["signal"],
    outputs: ["signal"],
    params: [{ name: "drive", label: "drive", ...t(1, 30, 0.1, 4) }],
  },
  [Kind.Vca]: {
    kind: Kind.Vca,
    label: "VCA",
    color: "#22c55e",
    inputs: ["signal", "gain cv"],
    outputs: ["signal"],
    params: [],
  },
  [Kind.Mixer]: {
    kind: Kind.Mixer,
    label: "Mixer",
    color: "#eab308",
    inputs: ["a", "b", "c"],
    outputs: ["out"],
    params: [],
  },
  [Kind.Constant]: {
    kind: Kind.Constant,
    label: "Constant",
    color: "#64748b",
    inputs: [],
    outputs: ["value"],
    params: [{ name: "value", label: "value", ...t(-2, 2, 0.01, 0.5) }],
  },
  [Kind.Out]: {
    kind: Kind.Out,
    label: "Output",
    color: "#ef4444",
    inputs: ["signal"],
    outputs: [],
    params: [],
  },
  [Kind.Midi]: {
    kind: Kind.Midi,
    label: "MIDI / CV",
    color: "#ec4899",
    inputs: [],
    outputs: ["gate", "pitch cv"],
    params: [],
  },
};

export const PALETTE_ORDER: KindCode[] = [
  Kind.Midi,
  Kind.Osc,
  Kind.Adsr,
  Kind.Filter,
  Kind.Distortion,
  Kind.Vca,
  Kind.Mixer,
  Kind.Constant,
  Kind.Out,
];
