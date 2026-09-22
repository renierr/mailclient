//! The functions Dart can call.
//!
//! Everything public in this module is part of the FFI surface: adding,
//! renaming or re-typing anything here means re-running
//! `flutter_rust_bridge_codegen generate` (see `flutter/README.md`).
//!
//! ## Shape of the surface
//!
//! Large payloads cross as **JSON strings**, produced by `mailcore::feed` —
//! the same serialisation the Qt frontend consumes, so there is exactly one
//! definition of what a folder row or a reader payload contains, and Dart
//! models decode it. Small, structural things ([`events::JobEvent`],
//! [`init::AppInfo`]) cross as generated structs, where a typed value is
//! worth more than a parse.
//!
//! ## Errors
//!
//! Every fallible call returns `anyhow::Result`, which flutter_rust_bridge
//! turns into a thrown Dart exception carrying the message. There is no
//! `""`-means-ok convention here; that was a Qt-property artefact.

pub mod accounts;
pub mod attachments;
pub mod composer;
pub mod contacts;
pub mod events;
pub mod folders;
pub(crate) mod form;
pub mod init;
pub mod messages;
pub mod mutate;
pub mod search;
pub mod settings;
pub mod sync;
