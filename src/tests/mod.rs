#[allow(dead_code)]
pub(crate) mod utils;

mod common;

#[cfg(not(loom))]
mod mpmc;

#[cfg(not(loom))]
mod spmc;

#[cfg(not(loom))]
#[cfg(feature = "unordered_mpmc")]
mod unordered_mpmc;

#[cfg(loom)]
mod loom_test;