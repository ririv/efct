import { defineModule, pure } from "efct";

import { add } from "./project_math.js";

export const { addOne } = defineModule(import.meta.url, {
  addOne: pure()(function addOne(value: number): number {
    return add(value, 1);
  }),
});
