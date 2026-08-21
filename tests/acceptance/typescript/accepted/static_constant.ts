import { defineModule, pure } from "efct";

const INCREMENT: number = 1;

export const { increment } = defineModule(import.meta.url, {
  increment: pure()(function increment(value: number): number {
    return value + INCREMENT;
  }),
});
