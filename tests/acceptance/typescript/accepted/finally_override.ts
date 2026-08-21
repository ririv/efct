import { defineModule, pure } from "efct";

export const { recover } = defineModule(import.meta.url, {
  recover: pure()(function recover(): number {
    try {
      throw new RangeError("failure");
    } finally {
      return 1;
    }
  }),
});
