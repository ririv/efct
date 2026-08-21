import { defineModule, pure } from "@efct/efct";

export const { identity } = defineModule(import.meta.url, {
  identity: pure()(function identity(value) {
    return value;
  }),
});
