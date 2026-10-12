import { applyTax } from "./tax";

export interface Item {
  sku: string;
  qty: number;
  unitPrice: number;
}

// Regla: redondeo hacia arriba al céntimo.
export function roundPrice(value: number): number {
  return Math.ceil(value * 100) / 100;
}

export function priceOrder(items: Item[]): number {
  const subtotal = items.reduce((acc, i) => acc + i.qty * i.unitPrice, 0);
  return roundPrice(applyTax(subtotal));
}
