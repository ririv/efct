import { defineModule, effect, effects } from "efct";

export const { currentTime } = defineModule(import.meta.url, {
  currentTime: effects(effect.Clock())(function currentTime(): number {
    return Date.now();
  }),
});
