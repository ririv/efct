import { defineModule, pure } from "@efct/efct";

export const { fail } = defineModule(import.meta.url, {
  fail: pure()(function fail(): number {
    throw "failure";
  }),
});
