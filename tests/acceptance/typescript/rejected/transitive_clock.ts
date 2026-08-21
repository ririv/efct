import { defineModule, effect, effects, pure } from "efct";

export const { currentTime, hiddenClock } = defineModule(import.meta.url, {
  currentTime: effects(effect.Clock())(function currentTime(): number {
    return Date.now();
  }),
  hiddenClock: pure()(function hiddenClock(): number {
    return currentTime();
  }),
});
