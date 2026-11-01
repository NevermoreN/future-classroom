//! Closed MSI helper roles. The tracked supervisor owns the original child
//! capability; only the actual guardian Join/End/exit and reconciled effects
//! permit settlement. Image retirement follows the original broker exit.
use super::{native, LifecycleError, Result};
use std::time::{Duration, Instant};

pub(super) mod manifest;

/// One original budget for startup, authenticated IPC, transaction observation,
/// effect verification and cleanup. A timeout never grants a replacement owner.
pub const BROKER_BUDGET: Duration = Duration::from_secs(120);

/// A local request only. The sealed image, fixed source and actual native
/// admission/guard/effect proofs independently decide whether it may execute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupCommand {
    Install,
    Repair,
    Uninstall,
}
fn setup_command(args: &[String]) -> Option<SetupCommand> {
    match args {
        [one] if one == "install" => Some(SetupCommand::Install),
        [one] if one == "repair" => Some(SetupCommand::Repair),
        [one] if one == "uninstall" => Some(SetupCommand::Uninstall),
        _ => None,
    }
}

/// Runs a fixed local operation; no path/hash/PID/property/result is accepted.
/// A clean rollback of an unsuccessful installation still returns its native
/// failure. Native install0 followed by intent rollback is a business failure.
pub fn run_setup(command: SetupCommand) -> Result<u32> {
    use native::broker::controlled_owner::{self, Operation};
    let operation = match command {
        SetupCommand::Install => Operation::Install,
        SetupCommand::Repair => Operation::Repair,
        SetupCommand::Uninstall => Operation::Uninstall,
    };
    let result = controlled_owner::run_setup(operation)?;
    if result.installation_succeeded() || result.install_return != 0 {
        Ok(result.install_return)
    } else {
        Err(LifecycleError::Unsettled)
    }
}

/// GUI helper/outer Setup entry. Empty argv asks the user for a local intent;
/// one exact operation word executes directly. All other commands retain the
/// closed legacy grammar. A normal helper without a seal cannot run Setup.
pub fn run_entry(args: &[String]) -> Result<u32> {
    if args.is_empty() {
        native::broker::controlled_owner::setup_entry::interactive()
    } else if let Some(command) = setup_command(args) {
        run_setup(command)
    } else {
        run_helper(args).map(|()| 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Begin,
    Broker,
    Guardian(u32),
    Supervisor(u32),
    OwnedGuardian,
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
        [one] if one == "owned-guardian" => Ok(Role::OwnedGuardian),
        [transaction, command] if command == "guardian" || command == "supervisor" => {
            if transaction.is_empty()
                || transaction.len() > 10
                || !transaction.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(LifecycleError::Invalid);
            }
            match transaction.parse::<u32>() {
                Ok(value) if value != 0 => Ok(if command == "supervisor" {
                    Role::Supervisor(value)
                } else {
                    Role::Guardian(value)
                }),
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
/// literal `supervisor` command; its internally launched child uses `guardian`.
/// The helper reads only its own pinned image.
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
    fn setup_requests_have_only_three_exact_intents_and_no_authority_fields() {
        for (word, expected) in [
            ("install", SetupCommand::Install),
            ("repair", SetupCommand::Repair),
            ("uninstall", SetupCommand::Uninstall),
        ] {
            assert_eq!(setup_command(&args(&[word])), Some(expected));
            for suffix in [
                "C:\\arbitrary.msi",
                "--hash",
                "123",
                "effects=true",
                "commit",
            ] {
                assert_eq!(setup_command(&args(&[word, suffix])), None);
            }
        }
        for words in [
            vec![],
            vec!["Install"],
            vec!["--setup"],
            vec!["upgrade"],
            vec!["commit"],
            vec!["--pid", "123"],
        ] {
            assert_eq!(setup_command(&args(&words)), None);
        }
    }

    #[test]
    fn unsealed_original_test_image_cannot_enter_any_setup_route() {
        let _serial = crate::NATIVE_TEST_SERIAL
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // The ordinary Cargo test PE has no lifecycle overlay/seal. Exercise
        // actual current-image discovery: it must refuse before a GUI window,
        // maintenance gate, Pending, original G launch or native Begin exists.
        for words in [vec![], vec!["install"], vec!["repair"], vec!["uninstall"]] {
            assert!(run_entry(&args(&words)).is_err());
        }
    }

    #[test]
    fn role_grammar_has_no_external_authority_fields() {
        assert_eq!(role(&args(&["123", "guardian"])), Ok(Role::Guardian(123)));
        assert_eq!(
            role(&args(&["123", "supervisor"])),
            Ok(Role::Supervisor(123))
        );
        assert_eq!(role(&args(&["owned-guardian"])), Ok(Role::OwnedGuardian));
        for words in [
            vec!["guardian", "123"],
            vec!["0", "guardian"],
            vec!["-1", "guardian"],
            vec!["4294967296", "guardian"],
            vec!["begin", "--pid", "1"],
            vec!["commit", "--effects-reconciled"],
            vec!["rollback"],
            vec!["commit"],
            vec!["supervisor", "123"],
            vec!["0", "supervisor"],
            vec!["123", "supervisor", "--pid", "1"],
            vec!["repair"],
            vec!["upgrade"],
            vec!["uninstall"],
            vec!["owned-guardian", "C:\\arbitrary.msi"],
            vec!["owned-guardian", "--hash", "123"],
        ] {
            assert_eq!(role(&args(&words)), Err(LifecycleError::Invalid));
        }
    }
}
