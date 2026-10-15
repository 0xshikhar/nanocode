use std::cmp::Ordering;

use num_cmp::NumCmp;
use serde_json::Number;

pub(crate) fn compare(left: &Number, right: &Number) -> Ordering {
    if let Some(left) = left.as_i64() {
        if let Some(right) = right.as_i64() {
            return left.cmp(&right);
        }
        if let Some(right) = right.as_u64() {
            return i128::from(left).cmp(&i128::from(right));
        }
        return left.num_cmp(right.as_f64().unwrap()).unwrap();
    }
    if let Some(left) = left.as_u64() {
        if let Some(right) = right.as_i64() {
            return i128::from(left).cmp(&i128::from(right));
        }
        if let Some(right) = right.as_u64() {
            return left.cmp(&right);
        }
        return left.num_cmp(right.as_f64().unwrap()).unwrap();
    }
    if right.is_i64() || right.is_u64() {
        return compare(right, left).reverse();
    }
    left.as_f64()
        .unwrap()
        .partial_cmp(&right.as_f64().unwrap())
        .unwrap()
}

pub(crate) fn multiple_of(value: &Number, divisor: &Number) -> bool {
    let (mut numerator, value_exponent) = decimal_parts(value);
    let (mut denominator, divisor_exponent) = decimal_parts(divisor);
    if numerator == 0 {
        return true;
    }
    let mut a = numerator;
    let mut b = denominator;
    while b != 0 {
        (a, b) = (b, a % b);
    }
    numerator /= a;
    denominator /= a;
    let shift = value_exponent - divisor_exponent;
    if shift >= 0 {
        for factor in [2, 5] {
            let mut available = shift;
            while available > 0 && denominator % factor == 0 {
                denominator /= factor;
                available -= 1;
            }
        }
        denominator == 1
    } else {
        if denominator != 1 {
            return false;
        }
        let mut required = -shift;
        while required > 0 && numerator % 10 == 0 {
            numerator /= 10;
            required -= 1;
        }
        required == 0
    }
}

fn decimal_parts(number: &Number) -> (u128, i32) {
    let text = number.to_string();
    let text = text.strip_prefix('-').unwrap_or(&text);
    let (mantissa, mut exponent) = match text.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().unwrap()),
        None => (text, 0),
    };
    let mut coefficient = 0;
    let mut fractional = false;
    for byte in mantissa.bytes() {
        if byte == b'.' {
            fractional = true;
        } else {
            coefficient = coefficient * 10 + u128::from(byte - b'0');
            exponent -= i32::from(fractional);
        }
    }
    (coefficient, exponent)
}

pub(crate) fn compare_count(count: usize, limit: &Number) -> Ordering {
    compare(&Number::from(count), limit)
}
