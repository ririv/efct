import { defineModule, pure } from "@efct/efct";

export const { currentTime } = defineModule(import.meta.url, {
  currentTime: pure()(function currentTime(): number {
    return Date.now();
  }),
});
