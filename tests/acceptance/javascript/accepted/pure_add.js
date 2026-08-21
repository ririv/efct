import { defineModule, pure } from "@efct/efct";

export const { add } = defineModule(import.meta.url, {
  add: pure()(
    /**
     * @param {number} left
     * @param {number} right
     * @returns {number}
     */
    function add(left, right) {
      return left + right;
    },
  ),
});
