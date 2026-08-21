from dataclasses import dataclass

from adapters import checkout_region, current_time_ns
from efct import effect, effects, partial, pure


@pure
@dataclass(frozen=True, slots=True)
class Order:
    sku: str
    unit_price_cents: int
    quantity: int
    customer_tier: str


@pure
@dataclass(frozen=True, slots=True)
class Invoice:
    sku: str
    region: str
    subtotal_cents: int
    discount_cents: int
    tax_cents: int
    total_cents: int


@pure()
def sample_order() -> Order:
    return Order("EFCT-MUG", 2500, 3, "member")


@pure()
def tax_cents(taxable_cents: int, region: str) -> int:
    if region == "tax-free":
        return 0
    if region == "reduced":
        return taxable_cents // 20
    return taxable_cents * 33 // 400


@pure()
def calculate_invoice(order: Order, region: str, current_time: int) -> Invoice:
    subtotal_cents = order.unit_price_cents * order.quantity
    discount_cents = 0
    if order.customer_tier == "member" and current_time < 4102444800000000000:
        discount_cents = subtotal_cents // 10
    taxable_cents = subtotal_cents - discount_cents
    invoice_tax_cents = tax_cents(taxable_cents, region)
    return Invoice(
        order.sku,
        region,
        subtotal_cents,
        discount_cents,
        invoice_tax_cents,
        taxable_cents + invoice_tax_cents,
    )


@effects(
    effect.Console(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def show_invoice(invoice: Invoice) -> None:
    print("Order:", invoice.sku)
    print("Region:", invoice.region)
    print("Subtotal (cents):", invoice.subtotal_cents)
    print("Discount (cents):", invoice.discount_cents)
    print("Tax (cents):", invoice.tax_cents)
    print("Total (cents):", invoice.total_cents)


@effects(
    effect.Console(),
    effect.Clock(),
    effect.Environment(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def run() -> None:
    order = sample_order()
    region = checkout_region()
    current_time = current_time_ns()
    invoice = calculate_invoice(order, region, current_time)
    show_invoice(invoice)


_efct = effects(
    effect.Console(),
    effect.Clock(),
    effect.Environment(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)

run()
