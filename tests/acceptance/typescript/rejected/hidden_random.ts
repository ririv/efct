import { defineModule, pure } from "efct";

export const { randomValue } = defineModule(import.meta.url, {
  randomValue: pure()(function randomValue(): number {
    return Math.random();
  }),
});
