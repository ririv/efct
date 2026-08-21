import { defineModule, effect, effects } from "@efct/efct";

export const { currentTime, readCurrentTime } = defineModule(import.meta.url, {
  currentTime: effects(effect.Clock())(function currentTime(): number {
    return Date.now();
  }),
  readCurrentTime: effects(effect.Clock())(function readCurrentTime(): number {
    return currentTime();
  }),
});
