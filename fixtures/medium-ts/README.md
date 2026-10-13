# medium-ts

Servicio de pedidos. Ver `docs/architecture.md`.

Fixture mediano de archeology-doc con contradicciones sembradas:

- `docs/architecture.md` dice que el IVA es 12%; `src/services/tax.ts` usa 13%.
- El redondeo de precios está implementado distinto en `pricing.ts` y `utils/format.ts`.
- `src/__tests__/orders.test.ts` tiene un test desactivado.
- `config/default.json` declara `vatRate: 0.12`, distinto del código.
