import { defineModule, partial, pure } from "efct";

export const { requireNonNegative } = defineModule(import.meta.url, {
  requireNonNegative: pure(partial.Throw())(
    function requireNonNegative(value: number): number {
      if (value < 0) {
        throw "negative";
      }
      return value;
    },
  ),
});
