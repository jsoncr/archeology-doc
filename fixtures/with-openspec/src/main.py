"""App con specs OpenSpec parcialmente implementadas."""


def add(a, b):
    return a + b


def invoice_total(amount):
    # Contradice `billing/round-totals` (sembrado): trunca en vez de redondear.
    return int(amount * 100) / 100


# El requisito `billing/apply-discount` no está implementado (sembrado).
