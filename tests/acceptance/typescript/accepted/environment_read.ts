import { defineModule, effect, effects } from "efct";

export const { readHome } = defineModule(import.meta.url, {
  readHome: effects(effect.Environment())(function readHome(): string | undefined {
    return process.env.HOME;
  }),
});
