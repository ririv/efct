import { defineModule, effect, effects, partial } from "efct";

export const { greet } = defineModule(import.meta.url, {
  greet: effects(effect.Console(), partial.Throw())(function greet(name: string): void {
    console.log("Hello", name);
  }),
});
