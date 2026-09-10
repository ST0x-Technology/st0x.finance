//! Shared financial primitives used across the st0x workspace.
//!
//! This is a leaf crate with no dependencies on other st0x crates, providing
//! domain types that multiple crates need: `Symbol`, `FractionalShares`,
//! `Usdc`, `Usd`, `Positive`, `NonNegative`, `HasZero`, and `Id<Tag>`.

pub use rain_math_float::{Float, FloatError};

mod constraint;
mod id;
#[cfg(feature = "test-support")]
pub mod proptest;
mod shares;
mod symbol;
mod usd;
mod usdc;

pub use constraint::{NonNegative, NotNonNegative, NotPositive, Positive};
pub use id::{BlankIdError, Id};
pub use shares::{FractionalShares, SharesConversionError};
pub use symbol::{EmptySymbolError, Symbol};
pub use usd::{Usd, UsdToCentsError};
pub use usdc::{
    Usdc, UsdcConversionError, UsdcToCentsError, cents as usdc_cents, opt_cents as usdc_opt_cents,
};

fn float_integer_string(value: Float) -> Result<String, FloatError> {
    let formatted = value.format_with_scientific(false)?;
    Ok(formatted
        .split_once('.')
        .map_or(formatted.as_str(), |(integer, _)| integer)
        .to_owned())
}

/// Trait for types that have a zero value and can be compared to it.
///
/// Comparisons are fallible because the underlying Float EVM-based
/// operations can technically fail on malformed data.
pub trait HasZero: Sized + Copy {
    const ZERO: Self;

    fn is_zero(&self) -> Result<bool, FloatError>;
    fn is_negative(&self) -> Result<bool, FloatError>;
}

impl HasZero for Float {
    const ZERO: Self = Self::from_raw(alloy_primitives::B256::ZERO);

    fn is_zero(&self) -> Result<bool, FloatError> {
        Self::is_zero(*self)
    }

    fn is_negative(&self) -> Result<bool, FloatError> {
        self.lt(Self::ZERO)
    }
}

impl Positive<FractionalShares> {
    /// Converts to whole shares count, returning error if value has a
    /// fractional part or exceeds u64 range. Use this when the target
    /// API does not support fractional shares.
    ///
    /// # Errors
    ///
    /// Returns [`ToWholeSharesError::Fractional`] if the value has a
    /// fractional part, or [`ToWholeSharesError::Overflow`] if it
    /// exceeds `u64` range.
    pub fn to_whole_shares(self) -> Result<u64, ToWholeSharesError> {
        let inner = self.inner();
        if !inner.is_whole()? {
            return Err(ToWholeSharesError::Fractional(inner));
        }

        let integer_str = float_integer_string(inner.inner()).map_err(ToWholeSharesError::Float)?;
        integer_str
            .parse::<u64>()
            .map_err(|_| ToWholeSharesError::Overflow(inner))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToWholeSharesError {
    #[error("Cannot convert fractional shares {0} to whole shares")]
    Fractional(FractionalShares),
    #[error("Shares value {0} exceeds u64 range")]
    Overflow(FractionalShares),
    #[error("Float operation failed: {0}")]
    Float(FloatError),
}

impl From<FloatError> for ToWholeSharesError {
    fn from(error: FloatError) -> Self {
        Self::Float(error)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::B256;
    use rain_math_float_macro::float;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ComparisonFails;

    impl HasZero for ComparisonFails {
        const ZERO: Self = Self;

        fn is_zero(&self) -> Result<bool, FloatError> {
            Err(FloatError::InvalidHex("comparison failed".to_owned()))
        }

        fn is_negative(&self) -> Result<bool, FloatError> {
            Err(FloatError::InvalidHex("comparison failed".to_owned()))
        }
    }

    impl From<ComparisonFails> for Float {
        fn from(_: ComparisonFails) -> Self {
            Self::from_raw(B256::ZERO)
        }
    }

    #[test]
    fn positive_rejects_zero() {
        let error = Positive::new(FractionalShares::ZERO).unwrap_err();
        assert!(matches!(
            error,
            NotPositive::Constraint { value }
                if value == FractionalShares::ZERO
        ));
    }

    #[test]
    fn positive_rejects_negative() {
        let negative = FractionalShares::new(float!(-1));
        let error = Positive::new(negative).unwrap_err();
        assert!(matches!(
            error,
            NotPositive::Constraint { value } if value == negative
        ));
    }

    #[test]
    fn positive_accepts_positive_value() {
        let value = FractionalShares::new(float!(1));
        let positive = Positive::new(value).unwrap();
        assert_eq!(positive.inner(), value);
    }

    #[test]
    fn positive_preserves_comparison_failure() {
        let error = Positive::new(ComparisonFails).unwrap_err();
        assert!(matches!(
            error,
            NotPositive::Comparison {
                value: ComparisonFails,
                source: FloatError::InvalidHex(message),
            } if message == "comparison failed"
        ));
    }

    #[test]
    fn positive_deserialize_rejects_zero() {
        let result: Result<Positive<FractionalShares>, _> = serde_json::from_str("\"0\"");
        let error = result.unwrap_err();
        assert!(
            error.to_string().to_lowercase().contains("positive"),
            "expected 'positive' in error message, got: {error}"
        );
    }

    #[test]
    fn positive_deserialize_rejects_negative() {
        let result: Result<Positive<FractionalShares>, _> = serde_json::from_str("\"-1\"");
        let error = result.unwrap_err();
        assert!(
            error.to_string().to_lowercase().contains("positive"),
            "expected 'positive' in error message, got: {error}"
        );
    }

    #[test]
    fn positive_deserialize_accepts_positive() {
        let positive: Positive<FractionalShares> = serde_json::from_str("\"5.5\"").unwrap();
        assert!(positive.inner().inner().eq(float!(5.5)).unwrap());
    }

    #[test]
    fn to_whole_shares_succeeds_for_whole_number() {
        let positive = Positive::new(FractionalShares::new(float!(42))).unwrap();
        assert_eq!(positive.to_whole_shares().unwrap(), 42);
    }

    #[test]
    fn to_whole_shares_rejects_fractional() {
        let positive = Positive::new(FractionalShares::new(float!(1.5))).unwrap();
        let error = positive.to_whole_shares().unwrap_err();
        assert!(matches!(error, ToWholeSharesError::Fractional(_)));
    }

    #[test]
    fn to_whole_shares_handles_values_above_one_billion() {
        // Float::format() switches to scientific notation ("1e10") for
        // magnitudes above 1e9, so extracting the integer part before the
        // '.' produced the wrong value. A non-scientific formatter must be
        // used instead.
        let positive = Positive::new(FractionalShares::new(float!(10000000000))).unwrap();
        assert_eq!(positive.to_whole_shares().unwrap(), 10_000_000_000);
    }

    #[test]
    fn non_negative_accepts_zero() {
        let non_neg = NonNegative::new(FractionalShares::ZERO).unwrap();
        assert_eq!(non_neg.inner(), FractionalShares::ZERO);
    }

    #[test]
    fn non_negative_accepts_positive_value() {
        let value = FractionalShares::new(float!(5));
        let non_neg = NonNegative::new(value).unwrap();
        assert_eq!(non_neg.inner(), value);
    }

    #[test]
    fn non_negative_rejects_negative() {
        let negative = FractionalShares::new(float!(-1));
        let error = NonNegative::new(negative).unwrap_err();
        assert!(matches!(
            error,
            NotNonNegative::Constraint { value } if value == negative
        ));
    }

    #[test]
    fn non_negative_preserves_comparison_failure() {
        let error = NonNegative::new(ComparisonFails).unwrap_err();
        assert!(matches!(
            error,
            NotNonNegative::Comparison {
                value: ComparisonFails,
                source: FloatError::InvalidHex(message),
            } if message == "comparison failed"
        ));
    }

    #[test]
    fn non_negative_deserialize_accepts_zero() {
        let non_neg: NonNegative<FractionalShares> = serde_json::from_str("\"0\"").unwrap();
        assert!(non_neg.inner().is_zero().unwrap());
    }

    #[test]
    fn non_negative_deserialize_rejects_negative() {
        let result: Result<NonNegative<FractionalShares>, _> = serde_json::from_str("\"-1\"");
        let error = result.unwrap_err();
        assert!(
            error.to_string().to_lowercase().contains("non-negative"),
            "expected 'non-negative' in error message, got: {error}"
        );
    }

    #[test]
    fn non_negative_usd_accepts_zero() {
        let non_neg = NonNegative::new(Usd::ZERO).unwrap();
        assert!(non_neg.inner().is_zero().unwrap());
    }

    #[test]
    fn non_negative_usd_rejects_negative() {
        let negative = Usd::new(float!(-50));
        let error = NonNegative::new(negative).unwrap_err();
        assert!(matches!(
            error,
            NotNonNegative::Constraint { value } if value == negative
        ));
    }

    #[test]
    fn float_from_raw_all_bytes_are_valid() {
        use alloy_primitives::B256;

        // Float is a dense encoding: 224-bit signed coefficient (high bytes)
        // + 32-bit signed exponent (low bytes). Every possible B256 value
        // maps to a valid float — there are no invalid bit patterns.
        // This means from_raw can never produce a value that fails basic
        // operations like formatting or comparison.
        let patterns: Vec<B256> = vec![
            B256::from([0xff; 32]),
            B256::from([0x00; 32]),
            B256::from([0x80; 32]),
            B256::from([0xde; 32]),
        ];

        for bytes in patterns {
            let raw = Float::from_raw(bytes);
            assert_eq!(raw.get_inner(), bytes);

            // All basic operations succeed on any raw bytes.
            raw.format().unwrap();
            raw.is_zero().unwrap();
            raw.abs().unwrap();
            (raw + float!(0)).unwrap();
        }
    }

    #[test]
    fn float_zero_constant_is_numeric_zero() {
        assert!(Float::ZERO.is_zero().unwrap());
        assert!(!Float::ZERO.is_negative().unwrap());
    }
}
