import { defineModule, pure } from "efct";

export const { recurse } = defineModule(import.meta.url, {
  recurse: pure()(function recurse(value: number): number {
    return recurse(value);
  }),
});
