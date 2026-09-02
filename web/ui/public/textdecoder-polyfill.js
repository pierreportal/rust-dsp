// Minimal UTF-8 TextDecoder + TextEncoder polyfills for
// AudioWorkletGlobalScope, which (in Chromium) lacks both. The wasm-bindgen
// `--target web` glue constructs one of each at module load time (the encoder
// appears when any exported function takes a string arg, e.g. set_param);
// without these polyfills the worklet module throws before
// `registerProcessor` runs and the AudioWorkletNode cannot be created.
//
// Imported BEFORE `./pkg/web.js` so the globals are in place when the glue runs.

if (typeof globalThis.TextDecoder === "undefined") {
  globalThis.TextDecoder = class TextDecoder {
    constructor(label = "utf-8", options = {}) {
      this.fatal = options.fatal ?? false;
      this.ignoreBOM = options.ignoreBOM ?? false;
    }
    decode(input = new Uint8Array(0)) {
      const bytes =
        input instanceof ArrayBuffer ||
        (typeof SharedArrayBuffer !== "undefined" &&
          input instanceof SharedArrayBuffer)
          ? new Uint8Array(input)
          : input;
      let out = "";
      let i = 0;
      const len = bytes.length;
      while (i < len) {
        const b1 = bytes[i++];
        if (b1 < 0x80) {
          out += String.fromCharCode(b1);
        } else if (b1 < 0xc0) {
          if (this.fatal) throw new TypeError("Invalid UTF-8 sequence");
          out += "�";
        } else if (b1 < 0xe0) {
          const b2 = bytes[i++] & 0x3f;
          out += String.fromCharCode(((b1 & 0x1f) << 6) | b2);
        } else if (b1 < 0xf0) {
          const b2 = bytes[i++] & 0x3f;
          const b3 = bytes[i++] & 0x3f;
          out += String.fromCharCode(((b1 & 0x0f) << 12) | (b2 << 6) | b3);
        } else {
          const b2 = bytes[i++] & 0x3f;
          const b3 = bytes[i++] & 0x3f;
          const b4 = bytes[i++] & 0x3f;
          let cp = ((b1 & 0x07) << 18) | (b2 << 12) | (b3 << 6) | b4;
          cp -= 0x10000;
          out += String.fromCharCode(0xd800 | (cp >> 10), 0xdc00 | (cp & 0x3ff));
        }
      }
      return out;
    }
  };
}

if (typeof globalThis.TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    encode(s = "") {
      const bytes = [];
      for (let i = 0; i < s.length; i++) {
        let cp = s.codePointAt(i) ?? 0;
        if (cp > 0xffff) i++; // consume low surrogate of the pair
        if (cp < 0x80) bytes.push(cp);
        else if (cp < 0x800) bytes.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
        else if (cp < 0x10000)
          bytes.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
        else
          bytes.push(
            0xf0 | (cp >> 18),
            0x80 | ((cp >> 12) & 0x3f),
            0x80 | ((cp >> 6) & 0x3f),
            0x80 | (cp & 0x3f)
          );
      }
      return new Uint8Array(bytes);
    }
    encodeInto(s, dest) {
      const enc = this.encode(s);
      const n = Math.min(enc.length, dest.length);
      for (let i = 0; i < n; i++) dest[i] = enc[i];
      return { read: s.length, written: enc.length };
    }
  };
}
