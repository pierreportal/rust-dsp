import { NODE_SPECS, PALETTE_ORDER, type KindCode } from "../audio/nodeSpec";

interface Props {
  onAdd: (kind: KindCode) => void;
}

export function Palette({ onAdd }: Props) {
  return (
    <div className="palette">
      <div className="palette__title">Modules</div>
      {PALETTE_ORDER.map((kind) => {
        const spec = NODE_SPECS[kind];
        return (
          <button
            key={kind}
            className="palette__item"
            style={{ borderLeftColor: spec.color }}
            onClick={() => onAdd(kind)}
          >
            {spec.label}
          </button>
        );
      })}
    </div>
  );
}
