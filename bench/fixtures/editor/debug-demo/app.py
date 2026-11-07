"""A tiny order report, for the debugger."""


def total(order):
    return sum(line["qty"] * line["price"] for line in order["lines"])


def main():
    orders = [
        {"id": 1001, "customer": "Ada", "lines": [{"qty": 2, "price": 9.5}]},
        {"id": 1002, "customer": "Linus", "lines": [{"qty": 1, "price": 120.0}]},
    ]
    grand = 0.0
    for order in orders:
        grand += total(order)
    print(f"{len(orders)} orders, {grand:.2f} in total")


if __name__ == "__main__":
    main()
