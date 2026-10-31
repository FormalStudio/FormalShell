//! Authentication for the shell: the lock screen's PAM conversation and the
//! session's polkit agent.

pub mod keyring;
pub mod pam;
pub mod polkit;
