"""Fixture legacy: aplicación con contradicciones sembradas."""

from .billing import apply_discount


def main():
    print(apply_discount(100, 20))


if __name__ == "__main__":
    main()
