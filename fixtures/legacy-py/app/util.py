"""Utilidades varias."""


# Duplicación sembrada: otra regla de descuento distinta
def discount_v2(price, percent):
    # la misma regla de negocio implementada con límite 15% en otro sitio
    if percent > 15:
        percent = 15
    return price * (1 - percent / 100)
