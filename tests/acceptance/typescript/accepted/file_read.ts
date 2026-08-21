import { readFileSync } from "node:fs";
import { defineModule, effect, effects, partial } from "@efct/efct";

export const { readText } = defineModule(import.meta.url, {
  readText: effects(effect.File.Read(), partial.Throw())(
    function readText(path: string): string {
      return readFileSync(path, "utf8");
    },
  ),
});
