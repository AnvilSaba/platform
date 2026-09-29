mod admin;
pub(super) mod interactions;
pub(super) mod presentation;
pub(in crate::features::mcguildlink) mod repository;

pub use admin::block;

#[cfg(test)]
mod repository_tests;
