import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";

function localProxyHost(host: string | undefined): string {
  const value = String(host || "").trim();
  if (!value || value === "0.0.0.0" || value === "::") return "127.0.0.1";
  return value;
}

const backendHost = localProxyHost(process.env.CCCC_WEB_HOST);
const backendPort = Number(process.env.CCCC_WEB_PORT || 8848) || 8848;
const backendTarget = `http://${backendHost}:${backendPort}`;

export default defineConfig({
  plugins: [react()],
  base: "/ui/",
  resolve: {
    // Prefer the CJS build for xterm to avoid a minification bug that can break
    // the ESM build's `requestMode` handler (seen as `ReferenceError: i is not defined`).
    alias: [
      { find: "@", replacement: path.resolve(__dirname, "./src") },
      { find: /^@xterm\/xterm$/, replacement: "@xterm/xterm/lib/xterm.js" },
    ],
  },
  build: {
    outDir: "../src/cccc/ports/web/dist",
    emptyOutDir: true,
    // Keep the warning meaningful after explicit app chunking; 500 kB is the
    // default Vite threshold and is now too noisy for this bundle graph.
    chunkSizeWarningLimit: 520,
    rollupOptions: {
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
        // Split large deps into dedicated chunks to avoid oversized bundles.
        manualChunks(id) {
          const normalizedId = id.split(path.sep).join("/");

          if (normalizedId.includes("/src/pages/chat/")
            || normalizedId.includes("/src/components/messageBubble/")
            || normalizedId.includes("/src/components/MessageBubble.tsx")
            || normalizedId.includes("/src/components/VirtualMessageList.tsx")
            || normalizedId.includes("/src/hooks/useChatTab.ts")
            || normalizedId.includes("/src/hooks/useChatTab.tsx")) {
            return "chat-core";
          }

          if (normalizedId.includes("/src/components/app/AppBackground.tsx")
            || normalizedId.includes("/src/components/app/AppFeedback.tsx")
            || normalizedId.includes("/src/components/DropOverlay.tsx")) {
            return "app-chrome";
          }

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
          // Drag-and-drop
          if (/[\\/]node_modules[\\/]@dnd-kit[\\/]/.test(id)) return "dnd";
          // Floating UI
          if (/[\\/]node_modules[\\/]@floating-ui[\\/]/.test(id)) return "floating-ui";
          // Remaining third-party deps
          return "vendor";
        },
      },
    },
  },
  server: {
    proxy: {
      "/api": {
        target: backendTarget,
        changeOrigin: true,
        ws: true,
      },
    },
  },
});
