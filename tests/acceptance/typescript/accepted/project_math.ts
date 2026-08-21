import { defineModule, pure } from "efct";

export const { add } = defineModule(import.meta.url, {
  add: pure()(function add(left: number, right: number): number {
    return left + right;
  }),
});
