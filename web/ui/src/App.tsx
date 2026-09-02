import { useCallback, useEffect, useRef, useState } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  Controls,
  addEdge,
  useNodesState,
  useEdgesState,
  type Node,
  type Edge,
  type Connection,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";

import { audioEngine } from "./audio/audioEngine";
import { NODE_SPECS, Kind, type KindCode } from "./audio/nodeSpec";
import { ModuleNode, type ModuleNodeData } from "./nodes/ModuleNode";
import { Palette } from "./components/Palette";
import { Keyboard } from "./components/Keyboard";

type ModuleNode = Node<ModuleNodeData>;

const nodeTypes = { module: ModuleNode };

const parsePort = (handle?: string | null): number | null => {
  if (!handle) return null;
  const i = handle.indexOf("-");
  if (i < 0) return null;
  const n = parseInt(handle.slice(i + 1), 10);
  return Number.isNaN(n) ? null : n;
};

function AppInner() {
  const [nodes, setNodes, onNodesChange] = useNodesState<ModuleNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([]);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const idCounter = useRef(0);

  const nextId = () => idCounter.current++;

  const handleParamChange = useCallback(
    (id: string, name: string, value: number) => {
      setNodes((nds) =>
        nds.map((n) =>
          n.id === id
            ? { ...n, data: { ...n.data, params: { ...n.data.params, [name]: value } } }
            : n
        )
      );
      audioEngine.setParam(Number(id), name, value);
    },
    [setNodes]
  );

  const buildNode = useCallback(
    (kind: KindCode, x: number, y: number, id: number): ModuleNode => {
      const spec = NODE_SPECS[kind];
      const params: Record<string, number> = {};
      for (const p of spec.params) params[p.name] = p.default;
      audioEngine.addNode(id, kind);
      for (const p of spec.params) audioEngine.setParam(id, p.name, p.default);
      return {
        id: String(id),
        type: "module",
        position: { x, y },
        data: { kind, params, onParamChange: handleParamChange },
      };
    },
    [handleParamChange]
  );

  const addNode = useCallback(
    (kind: KindCode) => {
      const id = nextId();
      const node = buildNode(kind, 40 + (id % 5) * 60, 40 + (id % 5) * 60, id);
      setNodes((nds) => [...nds, node]);
    },
    [buildNode, setNodes]
  );

  const onConnect = useCallback(
    (conn: Connection) => {
      setEdges((eds) => addEdge({ ...conn, animated: true }, eds));
      const fromPort = parsePort(conn.sourceHandle);
      const toPort = parsePort(conn.targetHandle);
      if (fromPort != null && toPort != null && conn.source && conn.target) {
        audioEngine.connect(Number(conn.source), fromPort, Number(conn.target), toPort);
      }
    },
    [setEdges]
  );

  const onEdgesDelete = useCallback((deleted: Edge[]) => {
    for (const e of deleted) {
      const fromPort = parsePort(e.sourceHandle);
      const toPort = parsePort(e.targetHandle);
      if (fromPort != null && toPort != null) {
        audioEngine.disconnect(Number(e.source), fromPort, Number(e.target), toPort);
      }
    }
  }, []);

  const onNodesDelete = useCallback((deleted: Node[]) => {
    for (const n of deleted) audioEngine.removeNode(Number(n.id));
  }, []);

  // Init audio + load a default patch on mount.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        await audioEngine.init();
        await audioEngine.resume();
        if (cancelled) return;
        setReady(true);
        loadDefaultPatch();
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const loadDefaultPatch = () => {
    const midi = nextId();
    const osc = nextId();
    const adsr = nextId();
    const vca = nextId();
    const out = nextId();

    const built: ModuleNode[] = [
      buildNode(Kind.Midi, 0, 120, midi),
      buildNode(Kind.Osc, 260, 40, osc),
      buildNode(Kind.Adsr, 260, 240, adsr),
      buildNode(Kind.Vca, 520, 140, vca),
      buildNode(Kind.Out, 780, 140, out),
    ];

    const edge = (from: number, fromPort: number, to: number, toPort: number): Edge => ({
      id: `e${from}-${fromPort}-${to}-${toPort}`,
      source: String(from),
      sourceHandle: `out-${fromPort}`,
      target: String(to),
      targetHandle: `in-${toPort}`,
      animated: true,
    });

    const builtEdges: Edge[] = [
      edge(midi, 1, osc, 0), // pitch cv -> osc freq cv
      edge(midi, 0, adsr, 0), // gate -> adsr gate
      edge(osc, 0, vca, 0), // osc -> vca signal
      edge(adsr, 0, vca, 1), // env -> vca gain cv
      edge(vca, 0, out, 0), // vca -> out
    ];
    for (const e of builtEdges) {
      audioEngine.connect(Number(e.source), parsePort(e.sourceHandle)!, Number(e.target), parsePort(e.targetHandle)!);
    }

    setNodes(built);
    setEdges(builtEdges);
  };

  const midiNodeIds = nodes
    .filter((n) => n.data.kind === Kind.Midi)
    .map((n) => Number(n.id));

  const onNoteOn = useCallback(
    (note: number) => {
      audioEngine.resume();
      midiNodeIds.forEach((id) => audioEngine.noteOn(id, note));
    },
    [midiNodeIds]
  );
  const onNoteOff = useCallback(
    (note: number) => {
      void note;
      midiNodeIds.forEach((id) => audioEngine.noteOff(id));
    },
    [midiNodeIds]
  );

  if (error) {
    return (
      <div className="error-banner">
        <strong>Audio init failed.</strong>
        <div>{error}</div>
        <div className="error-banner__hint">
          If AudioWorklet is unavailable, serve the app over <code>http://localhost</code> (run
          <code> npm run dev</code> in <code>web/ui/</code>) — not file:// or a LAN IP.
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <Palette onAdd={addNode} />
      <div className="app__main">
        <div className="app__topbar">
          <span className="app__title">rust-dsp modular</span>
          <span className="app__status">{ready ? "audio ready" : "starting…"}</span>
        </div>
        <div className="app__canvas">
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onEdgesDelete={onEdgesDelete}
            onNodesDelete={onNodesDelete}
            nodeTypes={nodeTypes}
            fitView
          >
            <Background />
            <Controls />
          </ReactFlow>
        </div>
        <Keyboard midiNodeIds={midiNodeIds} onNoteOn={onNoteOn} onNoteOff={onNoteOff} />
      </div>
    </div>
  );
}

export default function App() {
  return (
    <ReactFlowProvider>
      <AppInner />
    </ReactFlowProvider>
  );
}
