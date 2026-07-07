import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    proxy: {
      "/v1": "http://localhost:4000",
      "/key": "http://localhost:4000",
      "/spend": "http://localhost:4000",
      "/global": "http://localhost:4000",
      "/org": "http://localhost:4000",
      "/team": "http://localhost:4000",
      "/user": "http://localhost:4000",
      "/health": "http://localhost:4000",
      "/model": "http://localhost:4000",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
