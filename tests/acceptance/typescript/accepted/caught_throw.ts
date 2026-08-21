import { defineModule, pure } from "efct";

export const { recover } = defineModule(import.meta.url, {
  recover: pure()(function recover(message: string): string {
    try {
      throw new Error(message);
    } catch {
      return "fallback";
    }
  }),
});
