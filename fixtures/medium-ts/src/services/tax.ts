// FIXME: la documentación dice 12%; el valor real en producción es 13%.
export const VAT_RATE = 0.13;

export function applyTax(amount: number): number {
  return amount * (1 + VAT_RATE);
}
