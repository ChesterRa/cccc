import { readFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const packageJson = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8")
) as { version?: string };

export default defineConfig({
  plugins: [react()],
  base: "/ui/",
  define: {
    __WEB_VERSION__: JSON.stringify(String(packageJson.version || "").trim()),
  },
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
        manualChunks(id) {
          if (!id.includes("node_modules")) return;
          // React core + libs that import react (must stay in the same chunk
          // to avoid circular cross-chunk dependencies during initialisation)
          if (/[\\/]node_modules[\\/](react|react-dom|zustand|@tanstack|scheduler)[\\/]/.test(id)) return "react-vendor";
          // xterm terminal
          if (/[\\/]node_modules[\\/]@xterm[\\/]/.test(id)) return "xterm";
          // Markdown rendering
          if (/[\\/]node_modules[\\/](markdown-it|mdurl|uc\.micro|entities|linkify-it)[\\/]/.test(id)) return "markdown";
          // i18n
          if (/[\\/]node_modules[\\/](i18next|react-i18next)[\\/]/.test(id)) return "i18n";
          // Drag-and-drop + floating UI
          if (/[\\/]node_modules[\\/](@dnd-kit|@floating-ui)[\\/]/.test(id)) return "ui-utils";
          // Remaining third-party deps
          return "vendor";
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
