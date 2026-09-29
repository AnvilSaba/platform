mod admin;
pub(super) mod blocking;
mod event_handler;
mod interactions;
pub(super) mod model;
pub(super) mod ports;
mod presentation;
pub(super) mod queries;
mod repository;

pub use admin::links;
pub use blocking::block;
pub use event_handler::AccountLinksEventHandler;

pub(super) const LIST_LINK_BUTTON_ID: &str = "list_link_button";

#[cfg(test)]
mod repository_tests;
#[cfg(test)]
mod tests;
