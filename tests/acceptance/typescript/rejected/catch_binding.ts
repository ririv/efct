import { defineModule, pure } from "efct";

export const { recover } = defineModule(import.meta.url, {
  recover: pure()(function recover(): string {
    try {
      throw "failure";
    } catch (error) {
      return String(error);
    }
  }),
});
