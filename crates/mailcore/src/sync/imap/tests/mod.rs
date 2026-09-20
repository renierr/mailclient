//! Mock-server regression tests for the `imap-next` migration.
//!
//! - [`protocol`]: capability guards, SELECT / MOVE / CHANGEDSINCE fallbacks,
//!   `BYE` handling, greeting refusal, NOOP health probing.
//! - [`ops`]: trash / move / bulk-move `Seen` enforcement, Junk shortcut,
//!   cross-account guard, attachment fetching.
//! - [`discovery`]: subtree + NAMESPACE discovery, parent folder creation.

mod discovery;
mod ops;
mod protocol;
