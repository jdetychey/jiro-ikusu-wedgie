//! Display helpers shared by the browser and the device. Nothing here is consensus: it is never
//! hashed, and replay never runs it.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::float_arithmetic,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::arithmetic_side_effects,
        clippy::indexing_slicing
    )
)]

pub mod dirty;
