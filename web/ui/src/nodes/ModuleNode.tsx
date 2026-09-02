import { Handle, Position, type NodeProps } from "@xyflow/react";
import { NODE_SPECS, type KindCode } from "../audio/nodeSpec";

export interface ModuleNodeData {
  kind: KindCode;
  params: Record<string, number>;
  onParamChange: (id: string, name: string, value: number) => void;
  // `as` cast helper for react-flow's loose data typing.
  [key: string]: unknown;
}

export function ModuleNode({ id, data }: NodeProps) {
  const d = data as unknown as ModuleNodeData;
  const spec = NODE_SPECS[d.kind];

  return (
    <div className="module-node" style={{ borderColor: spec.color }}>
      <div className="module-node__title" style={{ background: spec.color }}>
        {spec.label}
      </div>

      <div className="module-node__body">
        <div className="module-node__ports module-node__ports--in">
          {spec.inputs.map((label, i) => (
            <div className="port-row" key={i}>
              <Handle
                id={`in-${i}`}
                type="target"
                position={Position.Left}
                style={{ top: `${(i + 0.5) * 22 + 34}px` }}
              />
              <span className="port-label">{label}</span>
            </div>
          ))}
        </div>

        <div className="module-node__ports module-node__ports--out">
          {spec.outputs.map((label, i) => (
            <div className="port-row port-row--out" key={i}>
              <span className="port-label">{label}</span>
              <Handle
                id={`out-${i}`}
                type="source"
                position={Position.Right}
                style={{ top: `${(i + 0.5) * 22 + 34}px` }}
              />
            </div>
          ))}
        </div>
      </div>

      {spec.params.length > 0 && (
        <div className="module-node__params">
          {spec.params.map((p) => (
            <label className="param" key={p.name}>
              <div className="param__head">
                <span>{p.label}</span>
                <span className="param__val">{d.params[p.name]?.toFixed(p.step < 1 ? 2 : 0)}</span>
              </div>
              <input
                type="range"
                min={p.min}
                max={p.max}
                step={p.step}
                value={d.params[p.name] ?? p.default}
                onChange={(e) => d.onParamChange(id, p.name, parseFloat(e.target.value))}
              />
            </label>
          ))}
        </div>
      )}
    </div>
  );
}
