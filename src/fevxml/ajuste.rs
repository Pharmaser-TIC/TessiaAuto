//! The invoice value readers: `read_total` / `numero_convenio`, the value check
//! of `validarYConciliarValorAntesJson` (FEV `verificar`).

use std::sync::LazyLock;

use regex::Regex;
use rust_decimal::Decimal;

use super::text::{decode, group, java_trim, re};

/// Parses a Java `new BigDecimal(String)` literal (sign, digits with optional
/// `.`, optional exponent; no surrounding whitespace). Returns `None` when
/// invalid or when the value does not fit `Decimal` (96-bit mantissa, scale <= 28).
pub fn parse_decimal_literal(s: &str) -> Option<Decimal> {
    let (neg, rest) = match s.as_bytes().first()? {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    let rest = rest.replace('E', "e");
    let (mant, exp) = match rest.split_once('e') {
        Some((m, e)) => (m, Some(e)),
        None => (rest.as_str(), None),
    };
    let (int_part, frac_part) = mant.split_once('.').unwrap_or((mant, ""));
    let digits = format!("{int_part}{frac_part}");
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut scale = i64::try_from(frac_part.len()).ok()?;
    if let Some(e) = exp {
        if e.starts_with("+-") || e.starts_with("-+") {
            return None;
        }
        scale -= i64::from(e.parse::<i32>().ok()?);
    }
    let trimmed = digits.trim_start_matches('0');
    let mut unscaled: i128 = if trimmed.is_empty() {
        0
    } else {
        trimmed
            .parse::<u128>()
            .ok()
            .and_then(|u| i128::try_from(u).ok())?
    };
    if scale < 0 {
        let factor = 10i128.checked_pow(u32::try_from(-scale).ok()?)?;
        unscaled = unscaled.checked_mul(factor)?;
        scale = 0;
    }
    if neg {
        unscaled = -unscaled;
    }
    Decimal::try_from_i128_with_scale(unscaled, u32::try_from(scale).ok()?).ok()
}

static CDATA_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"<cbc:Description>\s*<!\[CDATA\[(.*?)\]\]>\s*</cbc:Description>"));
static TOTAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"<cac:LegalMonetaryTotal(?-u:\b)[^>]*>.*?<cbc:(?:PayableAmount|TaxInclusiveAmount)(?-u:\b)[^>]*>\s*([0-9.,]+)\s*</cbc:(?:PayableAmount|TaxInclusiveAmount)>.*?</cac:LegalMonetaryTotal>",
    )
});

/// Invoice value of `extraerValorTotalXmlFactura`: the first PayableAmount or
/// TaxInclusiveAmount (whichever comes first) of the LegalMonetaryTotal inside
/// the CDATA invoice (or the whole XML without CDATA). A value with both `,`
/// and `.` loses its commas; a lone `,` is the decimal separator.
pub fn read_total(b: &[u8]) -> Option<Decimal> {
    let full = decode(b);
    let s = CDATA_RE
        .captures(&full)
        .map(|m| group(&m, 1).to_owned())
        .unwrap_or(full);
    let m = TOTAL_RE.captures(&s)?;
    let v = java_trim(group(&m, 1));
    let v = if v.contains(',') && v.contains('.') {
        v.replace(',', "")
    } else if v.contains(',') {
        v.replace(',', ".")
    } else {
        v.to_owned()
    };
    parse_decimal_literal(&v)
}

/// Parses a convenio "vlr. total" like `numeroConvenio`: `$` and spaces are
/// removed; with both separators `.` is thousands and `,` the decimal; a lone
/// `,` is the decimal.
pub fn numero_convenio(s: &str) -> Option<Decimal> {
    let v = java_trim(s);
    if v.is_empty() {
        return None;
    }
    let v = v.replace(['$', ' '], "");
    let v = if v.contains(',') && v.contains('.') {
        v.replace('.', "").replace(',', ".")
    } else if v.contains(',') {
        v.replace(',', ".")
    } else {
        v
    };
    parse_decimal_literal(&v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_java_literals() {
        let p = |s: &str| parse_decimal_literal(s).map(|d| d.to_string());
        assert_eq!(p("1e3").as_deref(), Some("1000"));
        assert_eq!(p("007.10").as_deref(), Some("7.10"));
        assert_eq!(p("5.").as_deref(), Some("5"));
        assert_eq!(p(".5").as_deref(), Some("0.5"));
        assert_eq!(p("-20").as_deref(), Some("-20"));
        assert_eq!(p("12e-2").as_deref(), Some("0.12"));
        assert_eq!(p("1.2.3"), None);
        assert_eq!(p("abc"), None);
        assert_eq!(p(""), None);
        assert_eq!(p("1e"), None);
        assert_eq!(p(" 1"), None);
    }
}
