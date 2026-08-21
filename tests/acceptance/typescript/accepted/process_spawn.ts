import { spawnSync } from "node:child_process";
import { defineModule, effect, effects, partial } from "efct";

export const { run } = defineModule(import.meta.url, {
  run: effects(effect.Process(), partial.Throw(), partial.Diverge())(
    function run(command: string): void {
      spawnSync(command);
    },
  ),
});
