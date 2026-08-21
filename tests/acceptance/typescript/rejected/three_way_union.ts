import { defineModule, pure } from "efct";

export const { preserve } = defineModule(import.meta.url, {
  preserve: pure()(function preserve(
    value: number | null | undefined,
  ): number | null | undefined {
    return value;
  }),
});
