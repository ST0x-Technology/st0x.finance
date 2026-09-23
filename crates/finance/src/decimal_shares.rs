//! Decimal-scale-preserving share quantities for exact wire protocols.

use std::fmt::Display;
use std::str::FromStr;

use alloy_primitives::U256;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// A share quantity whose decimal scale is part of its wire representation.
///
/// Use this at protocol boundaries where `100.50` must remain `100.50`.
/// Convert to [`crate::FractionalShares`] only after the exact wire value has
/// crossed that boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecimalShares(#[serde(with = "rust_decimal::serde::str")] Decimal);

impl DecimalShares {
    /// Creates an exact share quantity from a decimal value.
    #[must_use]
    pub const fn new(value: Decimal) -> Self {
        Self(value)
    }

    /// Returns the scale-preserving decimal value.
    #[must_use]
    pub const fn inner(self) -> Decimal {
        self.0
    }

    /// Converts the quantity to an exact 18-decimal unsigned integer.
    ///
    /// # Errors
    ///
    /// Returns an error for negative values, arithmetic overflow, values that
    /// cannot be represented losslessly with 18 decimals.
    pub fn to_u256_18_decimals(self) -> Result<U256, DecimalSharesConversionError> {
        if self.0.is_sign_negative() {
            return Err(DecimalSharesConversionError::NegativeValue { value: self.0 });
        }

        let mantissa: u128 = self
            .0
            .mantissa()
            .try_into()
            .map_err(|_| DecimalSharesConversionError::NegativeValue { value: self.0 })?;
        let scale = self.0.scale();

        if scale > 18 {
            let divisor = 10_u128.pow(scale - 18);
            if !mantissa.is_multiple_of(divisor) {
                return Err(DecimalSharesConversionError::PrecisionLoss { value: self.0 });
            }

            return Ok(U256::from(mantissa / divisor));
        }

        U256::from(mantissa)
            .checked_mul(U256::from(10_u128.pow(18 - scale)))
            .ok_or(DecimalSharesConversionError::Overflow)
    }
}

impl FromStr for DecimalShares {
    type Err = rust_decimal::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

impl Display for DecimalShares {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

/// Errors converting exact decimal shares to an 18-decimal integer amount.
#[derive(Debug, thiserror::Error)]
pub enum DecimalSharesConversionError {
    /// The quantity is negative and cannot be represented by `U256`.
    #[error("share quantity cannot be negative: {value}")]
    NegativeValue { value: Decimal },
    /// Scaling the decimal value overflowed `U256`.
    #[error("share quantity overflowed while scaling to 18 decimals")]
    Overflow,
    /// The quantity has non-zero precision beyond 18 decimal places.
    #[error("share quantity cannot be represented losslessly with 18 decimals: {value}")]
    PrecisionLoss { value: Decimal },
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{U256, uint};

    use super::{DecimalShares, DecimalSharesConversionError};

    #[test]
    fn display_preserves_decimal_scale() {
        let quantity = "100.50".parse::<DecimalShares>().unwrap();

        assert_eq!(quantity.to_string(), "100.50");
    }

    #[test]
    fn serde_preserves_decimal_scale_as_a_string() {
        let quantity = "100.50".parse::<DecimalShares>().unwrap();

        assert_eq!(serde_json::to_string(&quantity).unwrap(), r#""100.50""#);
        assert_eq!(
            serde_json::from_str::<DecimalShares>(r#""100.50""#).unwrap(),
            quantity
        );
        assert!(serde_json::from_str::<DecimalShares>("100.50").is_err());
    }

    #[test]
    fn exact_18_decimal_conversion_succeeds() {
        let quantity = "0.123456789012345678".parse::<DecimalShares>().unwrap();

        assert_eq!(
            quantity.to_u256_18_decimals().unwrap(),
            uint!(123456789012345678_U256)
        );
    }

    #[test]
    fn conversion_accepts_large_quantity_that_overflows_decimal_when_scaled() {
        let quantity = "1000000000000".parse::<DecimalShares>().unwrap();

        assert_eq!(
            quantity.to_u256_18_decimals().unwrap(),
            uint!(1000000000000000000000000000000_U256)
        );
    }

    #[test]
    fn conversion_rejects_non_zero_precision_beyond_18_decimals() {
        let quantity = "0.0000000000000000001".parse::<DecimalShares>().unwrap();

        assert!(matches!(
            quantity.to_u256_18_decimals(),
            Err(DecimalSharesConversionError::PrecisionLoss { .. })
        ));
    }

    #[test]
    fn conversion_accepts_redundant_zeroes_beyond_18_decimals() {
        let quantity = "0.1000000000000000000".parse::<DecimalShares>().unwrap();

        assert_eq!(
            quantity.to_u256_18_decimals().unwrap(),
            U256::from(100_000_000_000_000_000_u128)
        );
    }

    #[test]
    fn conversion_rejects_negative_values() {
        let quantity = "-1".parse::<DecimalShares>().unwrap();

        assert!(matches!(
            quantity.to_u256_18_decimals(),
            Err(DecimalSharesConversionError::NegativeValue { .. })
        ));
    }
}
