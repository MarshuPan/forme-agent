//! Original-file tampering goes through the fixture's already locked handle.
use super::*;

struct Fixture {
    journal: SandboxLeaseJournal,
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let journal = SandboxLeaseJournal::open(&root.path().join(JOURNAL_FILE)).unwrap();
        Self { journal, root }
    }

    fn open_lease(&self, suffix: &str) -> (String, SandboxLeaseBinding) {
        let path = self.root.path().join(format!("job-{suffix}"));
        fs::create_dir(&path).unwrap();
        let binding = SandboxLeaseBinding::new(
            format!("job-{suffix}"),
            p::canonical_digest(&"resource-guard-plan").unwrap(),
            format!("forme-job-{suffix}"),
            &path,
            SandboxFileIdentity::new(1, [1; 16]).unwrap(),
        )
        .unwrap();
        let id = self.journal.begin_lease(binding.clone()).unwrap();
        (id, binding)
    }

    fn finish(&self, id: &str, binding: &SandboxLeaseBinding) {
        self.journal
            .record_process_tree_terminated(
                id,
                SandboxProcessTreeTermination {
                    schema_version: p::SchemaVersion(1),
                    job_id: binding.job_id.clone(),
                    plan_digest: binding.plan_digest.clone(),
                    appcontainer_identity: binding.appcontainer_identity.clone(),
                    root_exit_observed: true,
                    active_processes: 0,
                    observed_at: 10,
                },
            )
            .unwrap();
        self.journal.begin_appcontainer_removal(id).unwrap();
        self.journal
            .record_appcontainer_removed(id, &binding.appcontainer_identity, true)
            .unwrap();
        self.journal.begin_job_directory_removal(id).unwrap();
        fs::remove_dir(&binding.job_directory).unwrap();
        self.journal.record_job_directory_removed(id).unwrap();
        self.journal.record_cleanup_receipt(id).unwrap();
    }

    fn bytes(&self) -> Vec<u8> {
        let mut writer = self.journal.writer.lock().unwrap();
        let position = writer.file.stream_position().unwrap();
        writer.file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        writer.file.read_to_end(&mut bytes).unwrap();
        writer.file.seek(SeekFrom::Start(position)).unwrap();
        bytes
    }

    fn replace(&self, bytes: &[u8]) {
        let mut writer = self.journal.writer.lock().unwrap();
        writer.file.seek(SeekFrom::Start(0)).unwrap();
        writer.file.write_all(bytes).unwrap();
        writer.file.set_len(bytes.len() as u64).unwrap();
        writer.file.sync_all().unwrap();
    }

    fn rejects_without_repair(&self, bytes: &[u8]) {
        assert!(self
            .journal
            .with_quiescent_leases(&mut || panic!("invalid original lease bytes"))
            .is_err());
        assert!(self.journal.writer.lock().unwrap().poisoned);
        assert!(self.journal.verify_startup_recovery_complete().is_err());
        assert!(self.journal.record_cleanup_receipt("unrelated").is_err());
        assert_eq!(self.bytes(), bytes);
    }
}

#[test]
fn original_lease_guard_preserves_bytes_position_watermark_and_os_lock() {
    let f = Fixture::new();
    let (id, binding) = f.open_lease("complete");
    f.finish(&id, &binding);
    let bytes = f.bytes();
    let (sequence, head) = {
        let mut writer = f.journal.writer.lock().unwrap();
        writer.file.seek(SeekFrom::Start(7)).unwrap();
        (
            writer.projection.sequence,
            writer.projection.head().unwrap(),
        )
    };
    let mut callbacks = 0;
    f.journal
        .with_quiescent_leases(&mut || {
            callbacks += 1;
            assert!(f.journal.writer.try_lock().is_err());
            let other = OpenOptions::new()
                .read(true)
                .write(true)
                .open(f.root.path().join(JOURNAL_FILE))
                .unwrap();
            assert!(other.try_lock().is_err());
            Ok(())
        })
        .unwrap();
    assert_eq!(callbacks, 1);
    let mut writer = f.journal.writer.lock().unwrap();
    assert_eq!(writer.file.stream_position().unwrap(), 7);
    assert_eq!(writer.projection.sequence, sequence);
    assert_eq!(writer.projection.head().unwrap(), head);
    drop(writer);
    assert_eq!(f.bytes(), bytes);
}

#[test]
fn pending_lease_blocks_recovery_without_poisoning_normal_cleanup() {
    let f = Fixture::new();
    let (id, binding) = f.open_lease("pending");
    let bytes = f.bytes();
    assert!(f
        .journal
        .with_quiescent_leases(&mut || panic!("cleanup is unfinished"))
        .is_err());
    assert!(!f.journal.writer.lock().unwrap().poisoned);
    assert_eq!(f.bytes(), bytes);
    f.finish(&id, &binding);
    f.journal.verify_startup_recovery_complete().unwrap();
}

#[test]
fn lease_guard_refuses_poisoned_writer_and_preserves_callback_errors() {
    let f = Fixture::new();
    let bytes = f.bytes();
    let error = f
        .journal
        .with_quiescent_leases(&mut || Err(p::Error("fixture recovery refused".into())))
        .unwrap_err();
    assert_eq!(error.0, "fixture recovery refused");
    assert!(!f.journal.writer.lock().unwrap().poisoned);
    f.journal.verify_startup_recovery_complete().unwrap();
    f.journal.writer.lock().unwrap().poisoned = true;
    f.rejects_without_repair(&bytes);
}

#[test]
fn lease_guard_rejects_corruption_unterminated_records_and_unknown_schema() {
    for variant in 0..3 {
        let f = Fixture::new();
        let (id, binding) = f.open_lease("corrupt");
        f.finish(&id, &binding);
        let mut bytes = f.bytes();
        match variant {
            0 => bytes[0] = b'!',
            1 => {
                bytes.pop();
            }
            _ => {
                let mut lines = bytes.split_inclusive(|b| *b == b'\n');
                let first = lines.next().unwrap();
                let mut record: serde_json::Value = serde_json::from_slice(first).unwrap();
                record["schema_version"] = serde_json::json!(99);
                let mut changed = serde_json::to_vec(&record).unwrap();
                changed.push(b'\n');
                changed.extend_from_slice(&bytes[first.len()..]);
                bytes = changed;
            }
        }
        f.replace(&bytes);
        f.rejects_without_repair(&bytes);
    }
}

#[test]
fn lease_guard_rejects_valid_truncation_and_same_watermark_foreign_chain() {
    for foreign in [false, true] {
        let f = Fixture::new();
        let (id, binding) = f.open_lease("original");
        let prefix = f.bytes();
        f.finish(&id, &binding);
        let bytes = if foreign {
            let other = Fixture::new();
            let (other_id, other_binding) = other.open_lease("foreign");
            other.finish(&other_id, &other_binding);
            assert_eq!(
                f.journal.writer.lock().unwrap().projection.sequence,
                other.journal.writer.lock().unwrap().projection.sequence
            );
            other.bytes()
        } else {
            prefix
        };
        f.replace(&bytes);
        f.rejects_without_repair(&bytes);
    }
}

#[test]
fn lease_guard_rejects_extra_valid_facts_and_oversized_original() {
    for oversized in [false, true] {
        let f = Fixture::new();
        let original_projection = f.journal.writer.lock().unwrap().projection.clone();
        f.open_lease("extra");
        f.journal.writer.lock().unwrap().projection = original_projection;
        if oversized {
            f.journal
                .writer
                .lock()
                .unwrap()
                .file
                .set_len(MAX_JOURNAL_BYTES + 1)
                .unwrap();
            assert!(f.journal.verify_startup_recovery_complete().is_err());
            assert!(f.journal.writer.lock().unwrap().poisoned);
            assert_eq!(
                f.journal
                    .writer
                    .lock()
                    .unwrap()
                    .file
                    .metadata()
                    .unwrap()
                    .len(),
                MAX_JOURNAL_BYTES + 1
            );
        } else {
            let bytes = f.bytes();
            f.rejects_without_repair(&bytes);
        }
    }
}
