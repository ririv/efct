import { defineModule, pure } from "efct";

export const { defaultNumber } = defineModule(import.meta.url, {
  defaultNumber: pure()(function defaultNumber(value: number | null): number {
    if (value === null) {
      return 0;
    } else {
      return value;
    }
  }),
});
