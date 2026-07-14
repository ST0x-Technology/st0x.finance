//! Compiles as an external crate with only the dependencies documented for
//! direct `st0x-float-macro` consumers.

use alloy_primitives::B256;
use rain_math_float::Float;
use st0x_float_macro::{float, float_result};

#[test]
fn documented_dependencies_support_generated_paths() {
    let literal: Float = float!(1.25);
    let fallible_literal: Result<Float, _> = float_result!(2.5);

    assert_ne!(literal.get_inner(), B256::ZERO);
    assert!(fallible_literal.is_ok());
}
