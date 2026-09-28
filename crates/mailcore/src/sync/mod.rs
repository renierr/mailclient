//! Sync engine. All protocols implement [`traits::SyncProvider`];
//! sending goes through [`traits::MailSender`]. IMAP first, JMAP/POP3 later.

pub mod background;
pub mod headless;
pub mod imap;
pub mod pool;
pub mod sender;
pub mod traits;
