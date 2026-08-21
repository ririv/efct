import { defineModule, pure } from "efct";

export const { identity } = defineModule(import.meta.url, {
  identity: pure()(function identity(value) {
    return value;
  }),
});
