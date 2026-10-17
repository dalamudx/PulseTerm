import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    {
      name: "pulseterm-workspace-style-nonce",
      apply: "build",
      transformIndexHtml: {
        order: "post",
        handler(html, context) {
          if (
            context.filename !==
              fileURLToPath(new URL("./workspace.html", import.meta.url)) ||
            html.includes('id="terminal-style-nonce"')
          )
            return;
          // Vite extracts empty styles; add this runtime Tauri marker after extraction.
          return [
            {
              tag: "style",
              attrs: {
                id: "terminal-style-nonce",
                nonce: "__TAURI_STYLE_NONCE__",
              },
              children: "",
              injectTo: "head-prepend",
            },
          ];
        },
      },
    },
  ],
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  clearScreen: false,
  build: {
    rollupOptions: {
      input: {
        index: fileURLToPath(new URL("./index.html", import.meta.url)),
        workspace: fileURLToPath(new URL("./workspace.html", import.meta.url)),
      },
    },
  },
});
