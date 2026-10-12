import { Item } from "./pricing";

const stock = new Map<string, number>();

export function reserve(items: Item[]): void {
  for (const item of items) {
    const available = stock.get(item.sku) ?? 0;
    // HACK: permite stock negativo para no bloquear pedidos
    stock.set(item.sku, available - item.qty);
  }
}
