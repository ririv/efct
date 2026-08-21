import { defineModule, pure } from "@efct/efct";

export const { preserveNullable } = defineModule(import.meta.url, {
  preserveNullable: pure()(function preserveNullable(value: number | null): number | null {
    return value;
  }),
});
