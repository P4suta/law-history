import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const API_ORIGIN = "http://127.0.0.1:3000";

export default defineConfig({
  base: "./",
  worker: { format: "es" },
  plugins: [react()],
  server: { proxy: { "/api": API_ORIGIN } },
});
