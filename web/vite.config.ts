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
        // 将大型依赖单独打包，避免单个 chunk 过大
        manualChunks: {
          // xterm 终端库 (~400KB) - 单独打包避免 AgentTab chunk 过大
          xterm: ["@xterm/xterm", "@xterm/addon-fit"],
          // React 核心
          "react-vendor": ["react", "react-dom"],
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
