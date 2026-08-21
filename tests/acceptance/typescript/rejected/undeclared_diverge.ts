import { defineModule, pure } from "@efct/efct";

export const { spin } = defineModule(import.meta.url, {
  spin: pure()(function spin(): void {
    while (true) {}
  }),
});
