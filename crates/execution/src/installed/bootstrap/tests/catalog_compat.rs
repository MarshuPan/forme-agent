//! Schema-1 configuration remains a checked new-brain path, never a fallback.
use super::*;

fn after_observation(authority: &BootstrapAuthority) -> i64 {
    // Catalog facts include elapsed I/O time. Advance the injected clock from
    // that durable observation rather than assuming it finishes within 10 ms.
    authority.journal.lock().unwrap().state.last_at + 1
}

#[test]
fn old_configuration_lists_only_checked_empty_target_and_preserves_original_selection() {
    let (directory, config) = fixture();
    let authority = open(&config);
    let proof = authority
        .issue_proof(&admin(&directory), "proof:catalog-compat", NOW)
        .unwrap();
    let binding = session("claim:catalog-compat");
    reserve(&authority, &proof.proof, &binding, NOW).unwrap();
    let service = authority.authenticate_service(WEB).unwrap();
    let request = p::BootstrapCandidateCatalogRequestV2 {
        schema_version: p::SchemaVersion(2),
        session: p::BootstrapSessionBindingV2 {
            schema_version: p::SchemaVersion(2),
            operation_id: binding.operation_id.clone(),
            setup_digest: binding.setup_digest.clone(),
        },
    };
    let first = authority
        .candidate_catalog_v2(&service, request.clone(), NOW)
        .unwrap();
    assert_eq!(first.candidates.len(), 1);
    let new = &first.candidates[0];
    assert_eq!(new.kind, p::BootstrapCandidateKind::NewBrainTarget);
    assert_eq!(new.status, p::BootstrapCandidateStatus::Available);
    assert!(new.owner_ref.is_none());
    let next_at = after_observation(&authority);
    let next = authority
        .candidate_catalog_v2(&service, request.clone(), next_at)
        .unwrap();
    assert!(next.verification_epoch > first.verification_epoch);
    let selected_at = after_observation(&authority);
    choose(&authority, &binding, selected_at).unwrap();
    let query_at = after_observation(&authority);
    let before = authority.lock(query_at).unwrap().snapshot_for_test();
    assert_eq!(
        authority
            .candidate_catalog_v2(&service, request.clone(), query_at)
            .unwrap(),
        next
    );
    assert_eq!(
        authority.lock(query_at).unwrap().snapshot_for_test(),
        before
    );
    write_private(
        &config.new_brain_directory.join("retained-data"),
        b"not empty",
    );
    assert_eq!(
        authority
            .candidate_catalog_v2(&service, request, query_at)
            .unwrap_err()
            .0,
        Code::DataUnavailable
    );
    assert_eq!(
        authority.lock(query_at).unwrap().snapshot_for_test(),
        before
    );
}

#[test]
fn unavailable_historical_discovery_cannot_downgrade_to_a_new_brain_catalog() {
    let (directory, mut config) = fixture();
    config.historical_brain_directory = Some(directory.0.join("historical"));
    let authority = open(&config);
    let proof = authority
        .issue_proof(&admin(&directory), "proof:no-fallback", NOW)
        .unwrap();
    let binding = session("claim:no-fallback");
    reserve(&authority, &proof.proof, &binding, NOW).unwrap();
    let before = authority.lock(NOW).unwrap().snapshot_for_test();
    assert_eq!(
        authority
            .candidate_catalog_v2(
                &authority.authenticate_service(WEB).unwrap(),
                p::BootstrapCandidateCatalogRequestV2 {
                    schema_version: p::SchemaVersion(2),
                    session: p::BootstrapSessionBindingV2 {
                        schema_version: p::SchemaVersion(2),
                        operation_id: binding.operation_id,
                        setup_digest: binding.setup_digest
                    }
                },
                NOW
            )
            .unwrap_err()
            .0,
        Code::Unavailable
    );
    assert_eq!(authority.lock(NOW).unwrap().snapshot_for_test(), before);
    assert_eq!(
        std::fs::read_dir(&config.new_brain_directory)
            .unwrap()
            .count(),
        0
    );
}
