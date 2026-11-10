//! Authentication for the shell: the lock screen's PAM conversation, the
//! session's polkit agent, and the account and logind session both act for.

pub mod keyring;
pub mod pam;
pub mod polkit;
pub mod prompter;
pub mod user;
