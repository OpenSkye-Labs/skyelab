//! SkyeLab's mobile client: one set of GPUI screens, run by the iOS binary (`main.rs`) and the Android cdylib
//! (`android/`). A remote client only: it talks `skyelab-protocol` to a desktop environment through
//! `skyelab-client` and runs no agents.

pub mod app;
mod demo;
pub mod env_store;
pub mod remote;
pub mod source;
mod text;
pub mod thread_rows;
pub mod timeline_rows;
