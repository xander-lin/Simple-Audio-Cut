//! Native iced frontend for Simple Audio Cut.
//!
//! The crate is split into replaceable layers:
//! - [`model`] holds plain application state,
//! - [`services`] exposes every external dependency behind a trait,
//! - [`jobs`] runs blocking work off the UI thread,
//! - [`update`] is the only place state mutates,
//! - [`ui`] and [`waveform`] are pure view code.

pub mod app;
pub mod events;
pub mod jobs;
pub mod message;
pub mod model;
pub mod services;
pub mod theme;
pub mod ui;
pub mod update;
pub mod waveform;
