import { defineModule, pure } from "efct";

export const { fail } = defineModule(import.meta.url, {
  fail: pure()(function fail(): number {
    throw "failure";
  }),
});
