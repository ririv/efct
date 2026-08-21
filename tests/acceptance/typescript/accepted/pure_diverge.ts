import { defineModule, partial, pure } from "@efct/efct";

export const { spin } = defineModule(import.meta.url, {
  spin: pure(partial.Diverge())(function spin(): void {
    while (true) {}
  }),
});
