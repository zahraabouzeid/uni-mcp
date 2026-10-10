import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": import.meta.dirname },
  },
  test: {
    environment: "jsdom",
    include: ["{app,components,lib}/**/*.test.{ts,tsx}"],
    pool: "threads",
  },
});
