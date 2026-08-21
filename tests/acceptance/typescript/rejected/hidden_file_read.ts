import { readFileSync } from "node:fs";
import { defineModule, pure } from "efct";

export const { readText } = defineModule(import.meta.url, {
  readText: pure()(function readText(path: string): string {
    return readFileSync(path, "utf8");
  }),
});
