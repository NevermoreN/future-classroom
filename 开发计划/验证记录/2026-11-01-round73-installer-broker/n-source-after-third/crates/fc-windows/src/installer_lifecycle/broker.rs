//! Closed MSI helper entrypoints. Notifications are advisory, never authority to
//! end a transaction or clear Pending. The server derives its decision from its
//! pinned own-image manifest and actual effects, then requires the original
//! authenticated guardian's real Join/End and physical exit.
use super::{native, LifecycleError, Result};
use std::time::{Duration, Instant};

pub(super) mod manifest;

/// One original budget for startup, authenticated IPC, transaction observation,
/// effect verification and cleanup. A timeout never grants a replacement owner.
pub const BROKER_BUDGET: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Begin,
    Broker,
    Guardian(u32),
    RollbackNotice,
    CommitNotice,
}

fn role(args: &[String]) -> Result<Role> {
    match args {
        [one, target]
            if one == "begin"
                && !target.is_empty()
                && target.encode_utf16().count() < 1024
                && !target.contains('\0') =>
        {
            Ok(Role::Begin)
        }
        [one] if one == "broker" => Ok(Role::Broker),
        [one] if one == "rollback" => Ok(Role::RollbackNotice),
        [one] if one == "commit" => Ok(Role::CommitNotice),
        [transaction, guardian] if guardian == "guardian" => {
            if transaction.is_empty()
                || transaction.len() > 10
                || !transaction.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(LifecycleError::Invalid);
            }
            match transaction.parse::<u32>() {
                Ok(value) if value != 0 => Ok(Role::Guardian(value)),
                _ => Err(LifecycleError::Invalid),
            }
        }
        _ => Err(LifecycleError::Invalid),
    }
}

/// `args` excludes argv[0]. Begin's resolved main-file path is an untrusted fact,
/// compared only to the OS-resolved fixed installation destination; it cannot
/// select a path. No PID, SID, result, operation nonce or effects flag is admitted.
/// The engine puts its transaction handle BEFORE the
/// literal `guardian` command. The helper reads only its own pinned image.
pub fn run_helper(args: &[String]) -> Result<()> {
    let role = role(args)?; // malformed/unsupported commands cannot create state
    let deadline = Instant::now() + BROKER_BUDGET;
    native::broker::run(role, deadline, args.get(1).map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn role_grammar_has_no_external_authority_fields() {
        assert_eq!(role(&args(&["123", "guardian"])), Ok(Role::Guardian(123)));
        for words in [
            vec!["guardian", "123"],
            vec!["0", "guardian"],
            vec!["-1", "guardian"],
            vec!["4294967296", "guardian"],
            vec!["begin", "--pid", "1"],
            vec!["commit", "--effects-reconciled"],
            vec!["repair"],
            vec!["upgrade"],
            vec!["uninstall"],
        ] {
            assert_eq!(role(&args(&words)), Err(LifecycleError::Invalid));
        }
    }
}
