use serde::Deserialize;
use std::fmt::Debug;

use crate::{Float, FloatError, HasZero};

macro_rules! define_constraint {
    (
        $(#[$error_meta:meta])*
        $error:ident, $error_message:literal;
        $(#[$wrapper_meta:meta])*
        $wrapper:ident, $is_invalid:ident
    ) => {
        $(#[$error_meta])*
        #[derive(Debug, thiserror::Error)]
        pub enum $error<T> {
            #[error($error_message)]
            Constraint { value: T },
            #[error("failed to compare value with zero: {source}")]
            Comparison {
                value: T,
                #[source]
                source: FloatError,
            },
        }

        $(#[$wrapper_meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Hash, serde::Serialize)]
        #[serde(transparent)]
        pub struct $wrapper<T>(T);

        impl<T> $wrapper<T>
        where
            T: HasZero + Into<Float>,
        {
            /// Validates and wraps a constrained value.
            ///
            /// # Errors
            ///
            #[doc = concat!("Returns [`", stringify!($error), "`] if `value` violates the constraint or cannot be compared with zero.")]
            pub fn new(value: T) -> Result<Self, $error<T>> {
                match $is_invalid(value) {
                    Ok(true) => Err($error::Constraint { value }),
                    Ok(false) => Ok(Self(value)),
                    Err(source) => Err($error::Comparison { value, source }),
                }
            }

            /// Returns the validated inner value.
            pub const fn inner(self) -> T {
                self.0
            }
        }

        impl<'de, T> Deserialize<'de> for $wrapper<T>
        where
            T: Deserialize<'de> + HasZero + Into<Float> + Debug,
        {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = T::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }

        impl<T: std::fmt::Display> std::fmt::Display for $wrapper<T> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_constraint!(
    /// Value must be positive (greater than zero).
    NotPositive,
    "value must be positive, got {value:?}";
    /// Wrapper that guarantees the inner value is positive (greater than zero).
    ///
    /// Use this when an API requires strictly positive values, such as order quantities.
    Positive,
    is_not_positive
);

define_constraint!(
    /// Value must be non-negative (zero or greater).
    NotNonNegative,
    "value must be non-negative, got {value:?}";
    /// Wrapper that guarantees the inner value is non-negative (zero or greater).
    ///
    /// Use this when an API requires values that cannot be negative, such as
    /// available cash after subtracting a reserve.
    NonNegative,
    is_negative
);

fn is_not_positive<T: HasZero>(value: T) -> Result<bool, FloatError> {
    let zero = value.is_zero()?;
    let negative = value.is_negative()?;
    Ok(zero || negative)
}

fn is_negative<T: HasZero>(value: T) -> Result<bool, FloatError> {
    value.is_negative()
}
