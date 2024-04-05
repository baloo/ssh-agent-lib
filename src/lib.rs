#![doc = include_str!("../README.md")]
#![deny(missing_debug_implementations)]

pub mod proto;

#[cfg(feature = "agent")]
pub mod agent;
pub mod error;

#[cfg(feature = "agent")]
pub use async_trait::async_trait;

#[cfg(feature = "agent")]
pub use self::agent::Agent;
