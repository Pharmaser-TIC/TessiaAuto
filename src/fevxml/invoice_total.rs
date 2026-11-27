//! How the XML total of an invoice is stored in `factura_valor_xml`.

use rust_decimal::{Decimal, RoundingStrategy};

/// The total as `factura_valor_xml` keeps it, DECIMAL(18,2): two decimals,
/// half up (`"1234.50"`, `"12.35"`).
pub fn storage_amount(d: Decimal) -> String {
    let mut r = d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
    r.rescale(2);
    r.to_string()
}
