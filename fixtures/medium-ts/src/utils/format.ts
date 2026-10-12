// Duplica la regla de redondeo de services/pricing.ts, pero redondea al más cercano.
export function formatPrice(value: number): string {
  return (Math.round(value * 100) / 100).toFixed(2);
}
