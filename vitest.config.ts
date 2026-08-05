import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "happy-dom",
    include: ["tests/components/**/*.test.tsx"],
    restoreMocks: true,
  },
});
