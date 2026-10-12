"""Facturación.

Regla de negocio (documentada): el descuento máximo es 10%.
"""

# FIXME: la constante contradice la documentación (20% en código)
MAX_DISCOUNT = 0.20


def apply_discount(price, percent):
    if percent > MAX_DISCOUNT * 100:
        percent = MAX_DISCOUNT * 100
    return price * (1 - percent / 100)


# Código muerto sembrado
def unused_helper():
    pass  # deprecated, nadie lo llama
