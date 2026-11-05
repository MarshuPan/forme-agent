//! Original isolated product journals, using the actual sole writer handle.
use super::*;

#[test]
fn original_journal_evidence_reads_real_bytes_without_appending_or_moving_the_cursor() {
    let f = Fixture::new();
    let signed = f.review();
    let bytes = journal_bytes(&f.journal);
    let checkpoint = f.journal.checkpoint().unwrap();
    let position = f
        .journal
        .writer
        .lock()
        .unwrap()
        .file
        .stream_position()
        .unwrap();
    let actual = f
        .journal
        .legacy_journal_evidence(&signed.review.binding, 10_001)
        .unwrap();
    assert_eq!(actual.kind, p::LegacyReleaseEvidenceKind::OriginalJournal);
    assert_eq!(actual.artifact_digest, p::sha256_content_digest(&bytes));
    assert_eq!(
        actual.binding_digest,
        signed.review.binding.digest().unwrap()
    );
    assert_eq!(f.journal.checkpoint().unwrap(), checkpoint);
    assert_eq!(
        f.journal
            .writer
            .lock()
            .unwrap()
            .file
            .stream_position()
            .unwrap(),
        position
    );
    assert_eq!(journal_bytes(&f.journal), bytes);
    assert!(!f.journal.writer.lock().unwrap().poisoned);
}

#[test]
fn original_journal_prefix_survives_its_own_intent_but_not_a_committed_decision() {
    let f = Fixture::new();
    let mut review = f.review();
    let original = f
        .journal
        .legacy_journal_evidence(&review.review.binding, 10_001)
        .unwrap();
    review.review.evidence[0] = original.clone();
    let request = f.request(&review);
    f.journal
        .begin_legacy_recovery(request.clone(), review.clone(), 10_001)
        .unwrap();
    assert_eq!(
        f.journal
            .legacy_journal_evidence(&review.review.binding, 10_002)
            .unwrap(),
        original
    );
    f.journal
        .commit_legacy_recovery(
            verification(&request, f.journal.revision().unwrap(), 19_000),
            hash("independent final observation"),
            10_003,
        )
        .unwrap();
    assert!(f
        .journal
        .legacy_journal_evidence(&review.review.binding, 10_004)
        .is_err());
    assert!(!f.journal.writer.lock().unwrap().poisoned);
}

#[test]
fn original_journal_foreign_binding_and_expiry_reject_without_poisoning_valid_bytes() {
    let f = Fixture::new();
    let original = f.review().review.binding;
    let checkpoint = f.journal.checkpoint().unwrap();
    for mode in 0..7 {
        let mut binding = original.clone();
        match mode {
            0 => binding.owner = p::OwnerPrincipalRef("owner:other".into()),
            1 => binding.original_operation_id = "other:upgrade".into(),
            2 => binding.journal_head = hash("foreign checkpoint"),
            3 => binding.journal_seq += 1,
            4 => binding.original_command_digest = hash("foreign command"),
            5 => binding.original_release_ref = "other-release".into(),
            _ => binding.config_revision += 1,
        }
        assert!(
            f.journal.legacy_journal_evidence(&binding, 10_001).is_err(),
            "mode {mode}"
        );
        assert_eq!(f.journal.checkpoint().unwrap(), checkpoint);
    }
    assert!(f.journal.legacy_journal_evidence(&original, 9_999).is_err());
    assert!(f
        .journal
        .legacy_journal_evidence(&original, original.expires_at)
        .is_err());
    assert!(f.journal.legacy_journal_evidence(&original, 10_001).is_ok());
    assert!(!f.journal.writer.lock().unwrap().poisoned);
}

fn replace_actual_bytes(journal: &ProductJournal, bytes: &[u8]) {
    let mut writer = journal.writer.lock().unwrap();
    writer.file.seek(SeekFrom::Start(0)).unwrap();
    writer.file.write_all(bytes).unwrap();
    writer.file.set_len(bytes.len() as u64).unwrap();
    writer.file.sync_all().unwrap();
}

#[test]
fn original_journal_corruption_poisoning_prevents_any_further_decision_append() {
    let f = Fixture::new();
    let review = f.review();
    let request = f.request(&review);
    let mut bytes = journal_bytes(&f.journal);
    let changed = bytes.len() - 2;
    bytes[changed] ^= 1;
    replace_actual_bytes(&f.journal, &bytes);
    assert!(f
        .journal
        .legacy_journal_evidence(&review.review.binding, 10_001)
        .is_err());
    assert!(f.journal.writer.lock().unwrap().poisoned);
    assert!(f
        .journal
        .begin_legacy_recovery(request, review, 10_002)
        .is_err());
    assert_eq!(journal_bytes(&f.journal), bytes);
}

#[test]
fn original_journal_valid_truncation_is_detected_against_the_live_projection() {
    let f = Fixture::new();
    let binding = f.review().review.binding;
    let bytes = journal_bytes(&f.journal);
    let previous_end = bytes[..bytes.len() - 1]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .unwrap()
        + 1;
    replace_actual_bytes(&f.journal, &bytes[..previous_end]);
    assert!(f.journal.legacy_journal_evidence(&binding, 10_001).is_err());
    assert!(f.journal.writer.lock().unwrap().poisoned);
}

#[test]
fn original_journal_rejects_an_intent_for_another_report_without_poisoning_the_writer() {
    let f = Fixture::new();
    let original = f.review().review.binding;
    let mut other = f.review();
    other.review.binding.review_id = "review:other".into();
    let digest = other.review.binding.digest().unwrap();
    for item in &mut other.review.evidence {
        item.binding_digest = digest.clone();
    }
    let request = f.request(&other);
    f.journal
        .begin_legacy_recovery(request, other.clone(), 10_001)
        .unwrap();
    assert!(f
        .journal
        .legacy_journal_evidence(&original, 10_002)
        .is_err());
    assert!(f
        .journal
        .legacy_journal_evidence(&other.review.binding, 10_002)
        .is_ok());
    assert!(!f.journal.writer.lock().unwrap().poisoned);
}
