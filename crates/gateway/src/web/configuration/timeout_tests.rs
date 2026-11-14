use super::*;

fn request(action: p::ProductMaintenanceAction, deadline: i64) -> p::ProductMaintenanceRequest {
    p::ProductMaintenanceRequest {
        schema_version: p::SchemaVersion(1),
        operation_id: "maintenance:timeout-fixture".into(),
        expected_revision: 99,
        owner: p::OwnerPrincipalRef("owner:fixture".into()),
        action,
        requested_deadline: deadline,
    }
}

#[test]
fn response_wait_is_bounded_and_does_not_change_authority_or_replay() {
    let now = 1_000;
    let upgrade = p::ProductMaintenanceAction::Upgrade {
        release_id: "target-fixture".into(),
        release_digest: p::sha256_content_digest(b"target"),
        backup_ref: "backup:fixture".into(),
    };
    let rollback = p::ProductMaintenanceAction::RollbackCompatibleRelease {
        release_id: "source-fixture".into(),
        release_digest: p::sha256_content_digest(b"source"),
        backup_ref: "backup:fixture".into(),
    };
    for action in [upgrade, rollback] {
        for (deadline, expected) in [(121_000, 130), (i64::MAX, 310), (now, 10), (1, 10)] {
            let command = request(action.clone(), deadline);
            let before = command.digest().unwrap();
            assert_eq!(
                maintenance_response_timeout(&command, now),
                Duration::from_secs(expected)
            );
            assert_eq!(command.digest().unwrap(), before);
        }
    }
    for action in [
        p::ProductMaintenanceAction::StopCore {},
        p::ProductMaintenanceAction::StageSignedRelease {
            release_id: "target-fixture".into(),
            release_digest: p::sha256_content_digest(b"target"),
        },
    ] {
        assert_eq!(
            maintenance_response_timeout(&request(action, i64::MAX), now),
            REQUEST_TIMEOUT
        );
    }
}
