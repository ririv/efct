import { defineModule, pure } from "@efct/efct";

export const { add } = defineModule(import.meta.url, {
  add: pure()((left: number, right: number): number => left + right),
});
