//! Recovery must fail closed under damaged rows, receipts and session fences.
use super::*;

#[test]
fn proposal_recovery_rejects_corrupt_payload_and_mismatched_terminal_receipts() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let clock = now();
    let session = login(&store, clock);
    let authenticated = owner(&store, &session, clock);
    let command = proposal_command("proposal:receipt-key", "proposal:receipt-target");
    store
        .prepare_workbench_proposal(&authenticated, &command)
        .unwrap();
    let binding = p::canonical_digest(&(
        p::VerifiedPrincipal(authenticated.owner().0.clone()),
        &command,
    ))
    .unwrap();
    let terminal = p::WorkbenchCommandReceipt {
        schema_version: p::SchemaVersion(1),
        command: command.command.clone(),
        binding,
        status: p::WorkbenchCommandStatus::Confirmed,
        event: p::EventId("receipt:terminal".into()),
    };
    for invalid in [
        p::WorkbenchCommandReceipt {
            schema_version: p::SchemaVersion(2),
            ..terminal.clone()
        },
        p::WorkbenchCommandReceipt {
            command: p::IdempotencyKey("proposal:other".into()),
            ..terminal.clone()
        },
        p::WorkbenchCommandReceipt {
            binding: p::sha256_content_digest(b"wrong binding"),
            ..terminal
        },
    ] {
        assert!(matches!(
            store.finish_workbench_proposal(&authenticated, &command, &invalid),
            Err(AuthError::RecoveryRequired)
        ));
        assert_eq!(
            store.pending_workbench_proposals(&authenticated).unwrap(),
            vec![command.clone()]
        );
    }
    let mut corrupt = command.clone();
    corrupt.outcome = p::ProposalOutcome::Reject;
    corrupt.defer_until = None;
    store
        .database()
        .unwrap()
        .connection
        .execute(
            "UPDATE workbench_proposal_commands SET payload=?1 WHERE command=?2",
            rusqlite::params![serde_json::to_string(&corrupt).unwrap(), command.command.0],
        )
        .unwrap();
    assert!(matches!(
        store.pending_workbench_proposals(&authenticated),
        Err(AuthError::RecoveryRequired)
    ));
    assert!(matches!(
        store.prepare_workbench_proposal(&authenticated, &command),
        Err(AuthError::Conflict)
    ));
}

#[test]
fn proposal_recovery_survives_installed_session_fence_and_requires_fresh_login() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let clock = now();
    let session = login(&store, clock);
    let authenticated = owner(&store, &session, clock);
    let command = proposal_command("proposal:fenced-key", "proposal:fenced-target");
    store
        .prepare_workbench_proposal(&authenticated, &command)
        .unwrap();
    let revision = fence_installed_sessions(
        &directory.0,
        "installation:fixture",
        &signing_key().verifying_key().to_bytes(),
        authenticated.owner(),
        "maintenance:proposal-recovery",
        &p::sha256_content_digest(b"fixture maintenance fence"),
    )
    .unwrap();
    assert_eq!(revision.auth_epoch, authenticated.view.auth_epoch + 1);
    assert!(store.pending_workbench_proposals(&authenticated).is_err());
    assert!(store
        .prepare_workbench_proposal(&authenticated, &command)
        .is_err());
    drop(store);
    let store = open(&directory);
    let clock = now();
    let session = login(&store, clock);
    let authenticated = owner(&store, &session, clock);
    assert_eq!(
        store.pending_workbench_proposals(&authenticated).unwrap(),
        vec![command]
    );
}

#[test]
fn proposal_recovery_upgrades_v8_without_replacing_account_or_active_sessions() {
    let directory = Directory::new();
    let store = claimed(&directory);
    let clock = now();
    let session = login(&store, clock);
    store
        .database()
        .unwrap()
        .connection
        .execute_batch("DROP TABLE workbench_proposal_commands; PRAGMA user_version=8;")
        .unwrap();
    drop(store);
    let store = open(&directory);
    let authenticated = owner(&store, &session, now());
    assert_eq!(authenticated.owner().0, "owner:fixture");
    assert!(store
        .pending_workbench_proposals(&authenticated)
        .unwrap()
        .is_empty());
    store
        .prepare_workbench_proposal(
            &authenticated,
            &proposal_command("proposal:upgraded", "proposal:upgrade-target"),
        )
        .unwrap();
}
