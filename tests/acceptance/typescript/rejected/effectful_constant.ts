import { defineModule, pure } from "@efct/efct";

const START: number = Date.now();

export const { elapsed } = defineModule(import.meta.url, {
  elapsed: pure()(function elapsed(current: number): number {
    return current - START;
  }),
});
