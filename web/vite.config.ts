import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  base: "/ui/",
  resolve: {
    // Prefer the CJS build for xterm to avoid a minification bug that can break
    // the ESM build's `requestMode` handler (seen as `ReferenceError: i is not defined`).
    alias: [
      { find: /^@xterm\/xterm$/, replacement: "@xterm/xterm/lib/xterm.js" },
    ],
  },
  build: {
    outDir: "../src/cccc/ports/web/dist",
    emptyOutDir: true,
    rollupOptions: {
      output: {
        entryFileNames: "assets/index.js",
        chunkFileNames: "assets/chunk-[name].js",
        assetFileNames: "assets/[name][extname]",
        // Split large deps into dedicated chunks to avoid oversized bundles.
        manualChunks: {
          // xterm (~400KB): keep separate to avoid bloating AgentTab.
          xterm: ["@xterm/xterm", "@xterm/addon-fit"],
          // React core
          "react-vendor": ["react", "react-dom"],
          // Three.js 3D scene: keep separate, loaded on demand.
          "three-vendor": ["three", "@react-three/fiber", "@react-three/drei"],
        },
      },
    },
  },
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8848",
        changeOrigin: true,
        ws: true,
      },
    },
  },
});
