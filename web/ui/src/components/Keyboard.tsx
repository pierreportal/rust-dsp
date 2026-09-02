import { useEffect, useState } from "react";

// White keys A S D F G H J, black keys W E T Y U -> C4..B4.
const KEYS = [
  { note: 60, name: "C", key: "a" },
  { note: 61, name: "C#", key: "w", black: true },
  { note: 62, name: "D", key: "s" },
  { note: 63, name: "D#", key: "e", black: true },
  { note: 64, name: "E", key: "d" },
  { note: 65, name: "F", key: "f" },
  { note: 66, name: "F#", key: "t", black: true },
  { note: 67, name: "G", key: "g" },
  { note: 68, name: "G#", key: "y", black: true },
  { note: 69, name: "A", key: "h" },
  { note: 70, name: "A#", key: "u", black: true },
  { note: 71, name: "B", key: "j" },
];

interface Props {
  midiNodeIds: number[];
  onNoteOn: (note: number) => void;
  onNoteOff: (note: number) => void;
}

export function Keyboard({ midiNodeIds, onNoteOn, onNoteOff }: Props) {
  const [pressed, setPressed] = useState<Set<number>>(new Set());

  const press = (note: number) => {
    if (pressed.has(note)) return;
    setPressed((p) => new Set(p).add(note));
    onNoteOn(note);
  };
  const release = (note: number) => {
    if (!pressed.has(note)) return;
    setPressed((p) => {
      const n = new Set(p);
      n.delete(note);
      return n;
    });
    onNoteOff(note);
  };

  useEffect(() => {
    const keyToMidi = new Map(KEYS.map((k) => [k.key, k.note]));
    const down = (e: KeyboardEvent) => {
      if (e.repeat) return;
      const midi = keyToMidi.get(e.key.toLowerCase());
      if (midi !== undefined) press(midi);
    };
    const up = (e: KeyboardEvent) => {
      const midi = keyToMidi.get(e.key.toLowerCase());
      if (midi !== undefined) release(midi);
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  });

  // midiNodeIds is present so consumers can read it; the actual note routing
  // is done by the parent via onNoteOn/onNoteOff.
  void midiNodeIds;

  return (
    <div className="keyboard">
      {KEYS.map((k) => (
        <div
          key={k.note}
          className={`key${k.black ? " key--black" : ""}${pressed.has(k.note) ? " key--active" : ""}`}
          onMouseDown={() => press(k.note)}
          onMouseUp={() => release(k.note)}
          onMouseLeave={() => release(k.note)}
        >
          <span className="key__label">{k.name}</span>
          <span className="key__binding">{k.key}</span>
        </div>
      ))}
    </div>
  );
}
