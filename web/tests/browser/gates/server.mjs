// Fixture-only server: no API proxy, live CCCC process or provider connection.
import { createServer, loadConfigFromFile } from "vite-plus";

const loaded = await loadConfigFromFile({ command: "serve", mode: "development" });
const server = await createServer({
  ...loaded.config,
  configFile: false,
  server: {
    host: "127.0.0.1",
    port: 15559,
    strictPort: true,
    hmr: false,
    forwardConsole: false,
    proxy: {},
  },
});
server.middlewares.use((request, response, next) => {
  if (request.url?.startsWith("/api/")) {
    response.statusCode = 503;
    response.end("Browser fixture did not mock this API request");
  } else next();
});
await server.listen();
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.once(signal, async () => {
    await server.close();
    process.exit(0);
  });
}
