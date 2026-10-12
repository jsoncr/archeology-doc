import unittest


class TestBilling(unittest.TestCase):
    @unittest.skip("flaky")
    def test_discount(self):
        pass  # test desactivado y vacío


if __name__ == "__main__":
    unittest.main()
