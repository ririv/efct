import { defineModule, pure } from "@efct/efct";

export const { compute } = defineModule(import.meta.url, {
  compute: pure()(function compute(value: number, double: boolean): number {
    let result: number = value + 1;
    result = result * 2;
    return double ? result : value;
  }),
});
