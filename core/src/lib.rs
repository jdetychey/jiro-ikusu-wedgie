#![no_std]
#![deny(unsafe_code)]
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

pub mod boss;
pub mod capture;
pub mod consts;
pub mod error;
pub mod game;
pub mod grid;
pub mod hash;
pub mod hits;
pub mod journal;
pub mod keccak;
pub mod level;
pub mod minion;
pub mod player;
pub mod recorder;
pub mod replay;
pub mod seed;
pub mod types;

pub use error::Error;
