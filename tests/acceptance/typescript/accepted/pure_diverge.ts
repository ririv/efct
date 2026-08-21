import { defineModule, partial, pure } from "efct";

export const { spin } = defineModule(import.meta.url, {
  spin: pure(partial.Diverge())(function spin(): void {
    while (true) {}
  }),
});
