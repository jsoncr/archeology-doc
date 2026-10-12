import { describe, expect, it } from "vitest";
import { roundPrice } from "../services/pricing";

describe("roundPrice", () => {
  it("redondea hacia arriba", () => {
    expect(roundPrice(1.001)).toBe(1.01);
  });
});
