import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import fs from "node:fs";

// The wasm-pack output lives outside this Vite project at ../pkg. The
// AudioWorklet (`public/graph-processor.js`) imports `./pkg/web.js` and the
// main thread fetches `./pkg/web_bg.wasm`, so `/pkg` must be served. In dev
// we serve it with a middleware; on build we copy it into `dist/pkg`.
const pkgDir = path.resolve(__dirname, "..", "pkg");

const CONTENT_TYPES: Record<string, string> = {
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".wasm": "application/wasm",
  ".json": "application/json",
};

function servePkg(): Plugin {
  const handle = (req, res, next) => {
    const file = path.join(pkgDir, decodeURIComponent(req.url || "/"));
    fs.readFile(file, (err, data) => {
      if (err) {
        next();
        return;
      }
      const ext = path.extname(file);
      const ct = CONTENT_TYPES[ext];
      if (ct) res.setHeader("Content-Type", ct);
      res.end(data);
    });
  };
  return {
    name: "serve-rust-dsp-pkg",
    configureServer(server) {
      server.middlewares.use("/pkg", handle);
    },
    configurePreviewServer(server) {
      server.middlewares.use("/pkg", handle);
    },
    closeBundle() {
      const dest = path.resolve(__dirname, "dist", "pkg");
      fs.mkdirSync(dest, { recursive: true });
      for (const name of fs.readdirSync(pkgDir)) {
        fs.copyFileSync(path.join(pkgDir, name), path.join(dest, name));
      }
    },
  };
}

export default defineConfig({
  plugins: [react(), servePkg()],
  server: { host: "localhost", port: 5173 },
});
