//! SkyeLab's mobile client: one set of GPUI screens, run by the iOS binary (`main.rs`) and the Android cdylib
//! (`android/`). A remote client only: it will talk `skyelab-protocol` to a desktop environment and runs no agents.

pub mod app;
pub mod env_store;
pub mod source;
