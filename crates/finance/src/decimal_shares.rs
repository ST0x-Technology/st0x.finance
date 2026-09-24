//! Decimal-scale-preserving share quantities for exact wire protocols.

use std::fmt::Display;
use std::str::FromStr;

use alloy_primitives::U256;
use rain_math_float::{Float, FloatError};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, Error as DecimalError};
use serde::{Deserialize, Serialize};

use crate::FractionalShares;

/// Maximum decimal places supported by Alpaca's tokenization API.
pub const ALPACA_MAX_DECIMALS: u32 = 9;

/// A numeric share quantity that preserves its decimal scale for wire output.
///
/// Serde accepts and emits string tokens only because JSON numbers cannot
/// reliably preserve lexical scale. Parsing uses `rust_decimal`'s exact decimal
/// grammar: scientific notation is rejected, underscore separators are
/// accepted, and more than 28 fractional digits are rejected even when the
/// excess digits are zero.
///
/// Equality is numeric, matching `rust_decimal`: `100.50` equals `100.5`.
/// Use [`Self::has_same_wire_representation`] when lexical scale matters.
/// Convert to [`FractionalShares`] for arithmetic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecimalShares(Decimal);

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

    /// Reports whether both quantities serialize to the same wire string: same
    /// mantissa, scale, and sign, so `0` and `-0` differ.
    #[must_use]
    pub fn has_same_wire_representation(self, other: Self) -> bool {
        self.0.mantissa() == other.0.mantissa()
            && self.0.scale() == other.0.scale()
            && self.0.is_sign_negative() == other.0.is_sign_negative()
    }

    /// Converts this wire quantity directly to its Rain Float-backed form.
    ///
    /// # Errors
    ///
    /// Returns an error when Rain Float cannot represent the decimal value.
    pub fn to_fractional_shares(self) -> Result<FractionalShares, DecimalSharesConversionError> {
        Float::parse(self.0.to_string())
            .map(FractionalShares::new)
            .map_err(DecimalSharesConversionError::Float)
    }

    /// Converts a Rain Float-backed quantity to its canonical decimal spelling.
    ///
    /// # Errors
    ///
    /// Returns an error when the value cannot be represented by `rust_decimal`.
    pub fn from_fractional_shares(
        value: FractionalShares,
    ) -> Result<Self, DecimalSharesConversionError> {
        Decimal::from_str_exact(&value.to_string())
            .map(Self)
            .map_err(DecimalSharesConversionError::Decimal)
    }

    /// Truncates toward zero to `decimals` decimal places using Rain Float
    /// arithmetic.
    ///
    /// Returns the retained wire quantity and the discarded dust. Their sum is
    /// numerically equal to the original quantity. Issuance's checked
    /// `rust_decimal` formula runs alongside: it supplies the wire scale and
    /// the overflow bound, and any disagreement with the Rain Float result is
    /// an error.
    ///
    /// # Errors
    ///
    /// Returns an error when `10^decimals` or the scaled quantity exceeds the
    /// legacy quantity bound, Rain Float arithmetic fails, a result cannot fit
    /// in `rust_decimal`, or the two truncations disagree.
    pub fn truncate_to_decimals(
        self,
        decimals: u32,
    ) -> Result<(Self, Self), DecimalSharesConversionError> {
        let power = 10_u64
            .checked_pow(decimals)
            .ok_or(DecimalSharesConversionError::ArithmeticOverflow)?;
        let multiplier = Decimal::from(power);
        let legacy_truncated = self
            .0
            .checked_mul(multiplier)
            .ok_or(DecimalSharesConversionError::ArithmeticOverflow)?
            .trunc()
            .checked_div(multiplier)
            .ok_or(DecimalSharesConversionError::ArithmeticOverflow)?;
        let legacy_dust = self
            .0
            .checked_sub(legacy_truncated)
            .ok_or(DecimalSharesConversionError::ArithmeticOverflow)?;

        let (truncated, dust) = self
            .to_fractional_shares()?
            .truncate_to_decimals(decimals)?;

        Ok((
            Self::from_fractional_shares(truncated)?.with_legacy_scale(legacy_truncated)?,
            Self::from_fractional_shares(dust)?.with_legacy_scale(legacy_dust)?,
        ))
    }

    /// Truncates to Alpaca's maximum supported precision and returns the dust.
    ///
    /// # Errors
    ///
    /// Returns an error when truncation or conversion fails.
    pub fn truncate_for_alpaca(self) -> Result<(Self, Self), DecimalSharesConversionError> {
        self.truncate_to_decimals(ALPACA_MAX_DECIMALS)
    }

    /// Converts the quantity to an exact 18-decimal unsigned integer.
    ///
    /// This intentionally preserves issuance's legacy `rust_decimal` range:
    /// values that overflow while scaling by `10^18` are rejected even if the
    /// mathematical result would fit in `U256`.
    ///
    /// # Errors
    ///
    /// Returns an error for negative values, Decimal overflow, or values that
    /// cannot be represented losslessly with 18 decimals.
    pub fn to_u256_18_decimals(self) -> Result<U256, DecimalSharesConversionError> {
        if self.0.is_sign_negative() {
            return Err(DecimalSharesConversionError::NegativeValue { value: self.0 });
        }

        let scaled = self
            .0
            .checked_mul(Decimal::from(10_u128.pow(18)))
            .ok_or(DecimalSharesConversionError::ArithmeticOverflow)?;

        if scaled.fract() != Decimal::ZERO {
            return Err(DecimalSharesConversionError::PrecisionLoss { value: scaled });
        }

        let integer = scaled
            .to_u128()
            .ok_or(DecimalSharesConversionError::ArithmeticOverflow)?;

        Ok(U256::from(integer))
    }

    /// Creates a wire quantity from a U256 value with 18 decimal places.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is outside `rust_decimal`'s range or the
    /// division cannot be represented.
    pub fn from_u256_18_decimals(value: U256) -> Result<Self, DecimalSharesConversionError> {
        let decimal: Decimal = value.to_string().parse()?;
        let quantity = decimal
            .checked_div(Decimal::from(10_u128.pow(18)))
            .ok_or(DecimalSharesConversionError::ArithmeticOverflow)?;

        Ok(Self(quantity))
    }

    /// Checks a Rain Float result against issuance's `Decimal` result and
    /// returns the latter, which carries the legacy wire scale.
    fn with_legacy_scale(self, legacy: Decimal) -> Result<Self, DecimalSharesConversionError> {
        if self.0 != legacy {
            return Err(DecimalSharesConversionError::TruncationMismatch {
                rain_float: self.0,
                decimal: legacy,
            });
        }

        Ok(Self(legacy))
    }
}

impl FromStr for DecimalShares {
    type Err = DecimalError;

    /// Parses a plain decimal string and keeps its scale.
    ///
    /// # Errors
    ///
    /// Returns [`DecimalError`] for scientific notation, more than 28
    /// fractional digits, values outside `rust_decimal`'s range, or other
    /// malformed input.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Decimal::from_str_exact(value).map(Self)
    }
}

impl Serialize for DecimalShares {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DecimalShares {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl Display for DecimalShares {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

/// Errors converting exact decimal shares at arithmetic boundaries.
#[derive(Debug, thiserror::Error)]
pub enum DecimalSharesConversionError {
    /// Decimal arithmetic exceeded the legacy quantity range.
    #[error("arithmetic overflow during share conversion")]
    ArithmeticOverflow,
    /// The quantity is negative and cannot be represented by `U256`.
    #[error("share quantity cannot be negative: {value}")]
    NegativeValue { value: Decimal },
    /// The scaled quantity has a fractional component.
    #[error("share quantity cannot be represented losslessly with 18 decimals: {value}")]
    PrecisionLoss { value: Decimal },
    /// Rain Float truncation disagrees with issuance's `Decimal` truncation.
    #[error("Rain Float truncation {rain_float} disagrees with decimal truncation {decimal}")]
    TruncationMismatch {
        rain_float: Decimal,
        decimal: Decimal,
    },
    /// Rain Float arithmetic or conversion failed.
    #[error("Rain Float share conversion failed: {0}")]
    Float(#[from] FloatError),
    /// The value cannot be represented by `rust_decimal`.
    #[error("decimal share conversion failed: {0}")]
    Decimal(#[from] DecimalError),
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy_primitives::{U256, uint};
    use proptest::prelude::*;
    use rain_math_float_macro::float;

    use crate::{Decimal, DecimalError, Float, FractionalShares};

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
            serde_json::from_str::<DecimalShares>(r#""100.50""#)
                .unwrap()
                .to_string(),
            "100.50"
        );
        let number_error = serde_json::from_str::<DecimalShares>("100.50").unwrap_err();
        assert_eq!(number_error.classify(), serde_json::error::Category::Data);
    }

    #[test]
    fn parser_contract_is_explicit() {
        let max_scale_zero = "0.0000000000000000000000000000"
            .parse::<DecimalShares>()
            .unwrap();
        assert_eq!(max_scale_zero.inner().scale(), 28);
        assert!(matches!(
            "0.00000000000000000000000000000".parse::<DecimalShares>(),
            Err(DecimalError::Underflow)
        ));
        assert!(matches!(
            "1e3".parse::<DecimalShares>(),
            Err(DecimalError::ErrorString(_))
        ));
        assert_eq!(
            "1_000.50".parse::<DecimalShares>().unwrap().to_string(),
            "1000.50"
        );
    }

    #[test]
    fn equality_is_numeric_while_wire_comparison_preserves_scale() {
        let two_places = "100.50".parse::<DecimalShares>().unwrap();
        let one_place = "100.5".parse::<DecimalShares>().unwrap();

        assert_eq!(two_places, one_place);
        assert!(!two_places.has_same_wire_representation(one_place));
        assert!(two_places.has_same_wire_representation(two_places));

        let positive_zero = DecimalShares::new(Decimal::ZERO);
        let negative_zero = DecimalShares::new(-Decimal::ZERO);
        assert_eq!(positive_zero, negative_zero);
        assert!(!positive_zero.has_same_wire_representation(negative_zero));
    }

    #[test]
    fn wire_comparison_distinguishes_scale_for_equal_mantissas() {
        let scaled = "1.00".parse::<DecimalShares>().unwrap();
        let whole = "100".parse::<DecimalShares>().unwrap();

        assert_eq!(scaled.inner().mantissa(), whole.inner().mantissa());
        assert!(!scaled.has_same_wire_representation(whole));
    }

    #[test]
    fn converts_directly_to_rain_float_backed_shares() {
        let quantity = "100.50".parse::<DecimalShares>().unwrap();

        assert_eq!(
            quantity.to_fractional_shares().unwrap(),
            FractionalShares::new(float!(100.5))
        );
    }

    #[test]
    fn converts_from_rain_float_backed_shares() {
        let quantity =
            DecimalShares::from_fractional_shares(FractionalShares::new(float!(100.5))).unwrap();

        assert_eq!(quantity, "100.5".parse().unwrap());
        assert_eq!(quantity.to_string(), "100.5");
    }

    #[test]
    fn rain_float_conversion_rejects_decimal_precision_loss() {
        let exact = FractionalShares::new(
            Float::parse("0.0000000000000000000000000001".to_owned()).unwrap(),
        );
        let inexact = FractionalShares::new(
            Float::parse("0.00000000000000000000000000025".to_owned()).unwrap(),
        );

        assert_eq!(
            DecimalShares::from_fractional_shares(exact).unwrap(),
            "0.0000000000000000000000000001".parse().unwrap()
        );
        assert!(matches!(
            DecimalShares::from_fractional_shares(inexact),
            Err(DecimalSharesConversionError::Decimal(_))
        ));
    }

    #[test]
    fn truncates_for_alpaca_and_returns_dust() {
        let original = "0.450574852280275235".parse::<DecimalShares>().unwrap();

        let (truncated, dust) = original.truncate_for_alpaca().unwrap();

        assert_eq!(truncated, "0.450574852".parse().unwrap());
        assert_eq!(dust, "0.000000000280275235".parse().unwrap());
        assert_eq!(
            (truncated.to_fractional_shares().unwrap() + dust.to_fractional_shares().unwrap())
                .unwrap(),
            original.to_fractional_shares().unwrap()
        );
    }

    #[test]
    fn truncation_matches_legacy_decimal_scale() {
        for value in ["1.100000000", "1.123456789123456789", "1.1"] {
            let original = Decimal::from_str(value).unwrap();
            let multiplier = Decimal::from(10_u64.pow(9));
            let legacy_truncated = (original * multiplier).trunc() / multiplier;
            let legacy_dust = original - legacy_truncated;
            let quantity = DecimalShares::new(original);

            let (truncated, dust) = quantity.truncate_for_alpaca().unwrap();

            assert_eq!(truncated.inner(), legacy_truncated);
            assert_eq!(truncated.inner().scale(), legacy_truncated.scale());
            assert_eq!(dust.inner(), legacy_dust);
            assert_eq!(dust.inner().scale(), legacy_dust.scale());
        }
    }

    proptest! {
        #[test]
        fn truncation_matches_legacy_decimal_value_and_scale(
            mantissa in -79_228_162_514_264_337_593_543_950_335_i128
                ..=79_228_162_514_264_337_593_543_950_335_i128,
            scale in 0_u32..=28,
            decimals in 0_u32..=19,
        ) {
            let original = Decimal::from_i128_with_scale(mantissa, scale);
            let multiplier = Decimal::from(10_u64.pow(decimals));
            let legacy = original
                .checked_mul(multiplier)
                .map(|scaled| scaled.trunc() / multiplier)
                .map(|truncated| (truncated, original - truncated));

            let result = DecimalShares::new(original).truncate_to_decimals(decimals);

            match legacy {
                Some((legacy_truncated, legacy_dust)) => {
                    let (truncated, dust) = result.unwrap();
                    prop_assert_eq!(truncated.to_string(), legacy_truncated.to_string());
                    prop_assert_eq!(dust.to_string(), legacy_dust.to_string());
                }
                None => prop_assert!(matches!(
                    result,
                    Err(DecimalSharesConversionError::ArithmeticOverflow)
                )),
            }
        }
    }

    #[test]
    fn legacy_scale_rejects_values_that_only_match_after_rounding() {
        let rain_float = "1.234".parse::<DecimalShares>().unwrap();
        let legacy = Decimal::from_str("1.23").unwrap();

        assert!(matches!(
            rain_float.with_legacy_scale(legacy),
            Err(DecimalSharesConversionError::TruncationMismatch { .. })
        ));
    }

    #[test]
    fn legacy_scale_copies_trailing_zeroes_for_equal_values() {
        let rain_float = "1.1".parse::<DecimalShares>().unwrap();
        let legacy = Decimal::from_str("1.10").unwrap();

        assert_eq!(
            rain_float.with_legacy_scale(legacy).unwrap().to_string(),
            "1.10"
        );
    }

    #[test]
    fn truncation_preserves_whole_numbers_and_zero() {
        for value in ["100", "0"] {
            let original = value.parse::<DecimalShares>().unwrap();
            let (truncated, dust) = original.truncate_for_alpaca().unwrap();

            assert_eq!(truncated, original);
            assert_eq!(dust, DecimalShares::default());
        }
    }

    #[test]
    fn truncation_rejects_legacy_multiplier_overflow() {
        let quantity = "1".parse::<DecimalShares>().unwrap();

        assert!(matches!(
            quantity.truncate_to_decimals(100),
            Err(DecimalSharesConversionError::ArithmeticOverflow)
        ));
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
    fn conversion_preserves_legacy_decimal_range() {
        let largest = "79228162514".parse::<DecimalShares>().unwrap();
        let overflowing = "79228162515".parse::<DecimalShares>().unwrap();

        assert_eq!(
            largest.to_u256_18_decimals().unwrap(),
            U256::from(79_228_162_514_000_000_000_000_000_000_u128)
        );
        assert!(matches!(
            overflowing.to_u256_18_decimals(),
            Err(DecimalSharesConversionError::ArithmeticOverflow)
        ));
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

    #[test]
    fn conversion_rejects_negative_zero_like_legacy_quantity() {
        let quantity = DecimalShares::new(-Decimal::ZERO);

        assert!(matches!(
            quantity.to_u256_18_decimals(),
            Err(DecimalSharesConversionError::NegativeValue { .. })
        ));
    }

    #[test]
    fn inverse_u256_conversion_preserves_precision() {
        let quantity =
            DecimalShares::from_u256_18_decimals(uint!(123456789012345678_U256)).unwrap();

        assert_eq!(quantity, "0.123456789012345678".parse().unwrap());
    }

    #[test]
    fn u256_conversion_roundtrips() {
        let original = "123.45".parse::<DecimalShares>().unwrap();
        let fixed = original.to_u256_18_decimals().unwrap();

        assert_eq!(
            DecimalShares::from_u256_18_decimals(fixed).unwrap(),
            original
        );
    }

    #[test]
    fn inverse_u256_conversion_rejects_values_outside_decimal_range() {
        assert!(matches!(
            DecimalShares::from_u256_18_decimals(U256::MAX),
            Err(DecimalSharesConversionError::Decimal(_))
        ));
    }

    #[test]
    fn decimal_constructor_keeps_numeric_equality() {
        let one = DecimalShares::new(Decimal::ONE);
        let scaled_one = DecimalShares::new(Decimal::from_str("1.00").unwrap());

        assert_eq!(one, scaled_one);
        assert!(!one.has_same_wire_representation(scaled_one));
    }
}
