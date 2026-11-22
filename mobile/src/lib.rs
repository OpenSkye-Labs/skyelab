//! SkyeLab's mobile client: one set of GPUI screens, run by the iOS binary (`main.rs`) and the Android cdylib
//! (`android/`). A remote client only: it talks `skyelab-protocol` to a desktop environment through
//! `skyelab-client` and runs no agents.

pub mod app;
pub mod env_store;
pub mod remote;
pub mod source;
