use super::*;
use crate::installed::desktop_output_fixture as fixture;
use std::fs;

#[test]
fn complete_png_is_preserved_exactly_and_cleaned_only_after_sealing() {
    let root = tempfile::tempdir().unwrap();
    let (request, intent, transfer) = fixture::request();
    transfer.validate_intent(&intent).unwrap();
    let bytes = fixture::png();
    let header = fixture::header(&transfer, &bytes);
    let mut sink =
        DesktopOutputAssembler::create(root.path(), transfer.clone(), request.clone()).unwrap();
    assert!(DesktopOutputAssembler::create(root.path(), transfer, request.clone()).is_err());
    assert!(sink.verified_bytes().is_err());
    sink.begin(header.clone()).unwrap();
    let split = bytes.len() / 2;
    for (offset, part) in [(0, &bytes[..split]), (split as u64, &bytes[split..])] {
        sink.write_chunk(fixture::chunk(&header, part, offset), part)
            .unwrap();
    }
    assert!(sink.verified_bytes().is_err());
    sink.finish(fixture::finish(&request, &header)).unwrap();
    assert_eq!(sink.verified_bytes().unwrap(), bytes);
    assert_eq!(sink.header().unwrap(), &header);
    assert_eq!(
        sink.receipt().unwrap().outcome,
        p::DesktopActionOutcome::Committed
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
    sink.result_sealed().unwrap();
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn invalid_original_bytes_never_form_a_completed_image() {
    let original = fixture::png();
    crate::validate_desktop_png(&original, 2, 1).unwrap();
    assert!(crate::validate_desktop_png(&original, 3, 1).is_err());
    let mut corrupt = original.clone();
    corrupt[original.len() / 2] ^= 1;
    let mut trailing = original.clone();
    trailing.extend_from_slice(b"hidden-tail");
    for bytes in [
        Vec::new(),
        b"not-a-png".to_vec(),
        original[..original.len() - 1].to_vec(),
        corrupt,
        trailing,
    ] {
        let root = tempfile::tempdir().unwrap();
        let (request, _, transfer) = fixture::request();
        let header = fixture::header(&transfer, &bytes);
        let mut sink =
            DesktopOutputAssembler::create(root.path(), transfer, request.clone()).unwrap();
        let result = sink
            .begin(header.clone())
            .and_then(|()| sink.write_chunk(fixture::chunk(&header, &bytes, 0), &bytes))
            .and_then(|()| sink.finish(fixture::finish(&request, &header)));
        assert!(result.is_err());
        assert!(sink.verified_bytes().is_err());
        assert!(sink.receipt().is_err());
        assert!(sink.result_sealed().is_err());
    }
}

#[test]
fn wrong_binding_order_budget_and_digest_poison_the_original_transfer() {
    for case in 0..12 {
        let root = tempfile::tempdir().unwrap();
        let (request, _, transfer) = fixture::request();
        let bytes = fixture::png();
        let header = fixture::header(&transfer, &bytes);
        let mut sink =
            DesktopOutputAssembler::create(root.path(), transfer, request.clone()).unwrap();
        let mut wrong = header.clone();
        let result = match case {
            0 => {
                wrong.binding.owner_ref.0 = "owner:other".into();
                sink.begin(wrong)
            }
            1 => {
                wrong.binding.job_id = "job:other".into();
                sink.begin(wrong)
            }
            2 => {
                wrong.binding.nonce.0 = "nonce:other".into();
                sink.begin(wrong)
            }
            3 => {
                wrong.binding.session_generation += 1;
                sink.begin(wrong)
            }
            4 => {
                wrong.bytes = 4097;
                sink.begin(wrong)
            }
            5 => {
                wrong.media_type = "text/plain".into();
                sink.begin(wrong)
            }
            6 => {
                wrong.width += 1;
                sink.begin(wrong)
            }
            7 => {
                sink.begin(header.clone()).unwrap();
                sink.begin(header.clone())
            }
            8 => sink.write_chunk(fixture::chunk(&header, &bytes, 0), &bytes),
            9 => {
                sink.begin(header.clone()).unwrap();
                sink.write_chunk(fixture::chunk(&header, &bytes, 1), &bytes)
            }
            10 => {
                sink.begin(header.clone()).unwrap();
                let mut chunk = fixture::chunk(&header, &bytes, 0);
                chunk.chunk_digest = p::sha256_content_digest(b"changed");
                sink.write_chunk(chunk, &bytes)
            }
            _ => {
                sink.begin(header.clone()).unwrap();
                sink.finish(fixture::finish(&request, &header))
            }
        };
        assert!(result.is_err(), "case {case}");
        assert!(sink.begin(header.clone()).is_err());
        assert!(sink.verified_bytes().is_err());
    }
}

#[test]
fn output_metadata_is_closed_and_wrong_versions_cannot_be_smuggled() {
    let (request, _, transfer) = fixture::request();
    let bytes = fixture::png();
    let header = fixture::header(&transfer, &bytes);
    let chunk = fixture::chunk(&header, &bytes, 0);
    let finish = fixture::finish(&request, &header);
    macro_rules! reject_extra {
        ($value:expr,$kind:ty) => {{
            let mut json = serde_json::to_value($value).unwrap();
            json["unexpected"] = true.into();
            assert!(serde_json::from_value::<$kind>(json).is_err());
        }};
    }
    reject_extra!(&transfer, p::DesktopOutputTransfer);
    reject_extra!(&transfer.binding, p::DesktopExchangeBinding);
    reject_extra!(&header, p::DesktopOutputHeader);
    reject_extra!(&chunk, p::DesktopOutputChunkHeader);
    reject_extra!(&finish, p::DesktopOutputFinish);
    let mut wrong = header.clone();
    wrong.schema_version = p::SchemaVersion(1);
    assert!(wrong.validate_for(&transfer).is_err());
    let mut wrong = chunk;
    wrong.next_offset += 1;
    assert!(wrong.validate_for(&header, 0, &bytes).is_err());
    let mut wrong = finish;
    wrong.receipt.nonce.0 = "nonce:other".into();
    assert!(wrong.validate_for(&header, &request).is_err());
}

#[test]
fn crashed_or_poisoned_transfer_retains_a_single_slot_across_jobs() {
    let root = tempfile::tempdir().unwrap();
    let (request, _, transfer) = fixture::request();
    let bytes = fixture::png();
    let header = fixture::header(&transfer, &bytes);
    {
        let mut sink =
            DesktopOutputAssembler::create(root.path(), transfer.clone(), request.clone()).unwrap();
        sink.begin(header.clone()).unwrap();
        sink.write_chunk(fixture::chunk(&header, &bytes[..7], 0), &bytes[..7])
            .unwrap();
    }
    assert_eq!(
        fs::read(root.path().join("original.png")).unwrap(),
        bytes[..7]
    );
    let original = fs::read(root.path().join("transfer.json")).unwrap();
    let (request2, _, mut transfer2) = fixture::request();
    transfer2.binding.job_id = "job:another".into();
    transfer2.transfer_ref = format!(
        "desktop-transfer:{}",
        &transfer2.binding.digest().unwrap().0[7..]
    );
    assert!(OutputStage::available(root.path()).is_err());
    assert!(DesktopOutputAssembler::create(root.path(), transfer, request).is_err());
    assert!(DesktopOutputAssembler::create(root.path(), transfer2, request2).is_err());
    assert_eq!(
        fs::read(root.path().join("transfer.json")).unwrap(),
        original
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn deadline_end_and_chunk_overflow_are_final_for_the_sink() {
    for case in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let (request, _, transfer) = fixture::request();
        let bytes = fixture::png();
        let header = fixture::header(&transfer, &bytes);
        let mut sink =
            DesktopOutputAssembler::create(root.path(), transfer, request.clone()).unwrap();
        sink.begin(header.clone()).unwrap();
        if case == 0 {
            sink.chunks = p::DESKTOP_OUTPUT_MAX_CHUNKS;
        }
        if case == 1 {
            sink.transfer.binding.deadline = 1;
        }
        let mut chunk = fixture::chunk(&header, &bytes, 0);
        if case == 2 {
            chunk.finished = false;
        }
        if case < 3 {
            assert!(sink.write_chunk(chunk, &bytes).is_err());
        } else {
            sink.write_chunk(chunk, &bytes).unwrap();
            if case == 3 {
                let mut finish = fixture::finish(&request, &header);
                finish.receipt.observed_at = request.deadline + 1;
                assert!(sink.finish(finish).is_err());
            } else {
                sink.finish(fixture::finish(&request, &header)).unwrap();
                assert!(sink.finish(fixture::finish(&request, &header)).is_err());
            }
        }
        assert!(sink.verified_bytes().is_err());
        assert!(sink.result_sealed().is_err());
        drop(sink);
        assert!(OutputStage::available(root.path()).is_err());
    }
}

#[cfg(windows)]
#[test]
fn live_stage_holds_original_file_and_ancestors_until_handle_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let stage_root = root.path().join("stage");
    let (request, _, transfer) = fixture::request();
    let bytes = fixture::png();
    let header = fixture::header(&transfer, &bytes);
    let mut sink = DesktopOutputAssembler::create(&stage_root, transfer, request.clone()).unwrap();
    sink.begin(header.clone()).unwrap();
    sink.write_chunk(fixture::chunk(&header, &bytes, 0), &bytes)
        .unwrap();
    sink.finish(fixture::finish(&request, &header)).unwrap();
    assert!(fs::rename(&stage_root, root.path().join("moved")).is_err());
    assert!(fs::remove_file(stage_root.join("original.png")).is_err());
    assert!(fs::write(stage_root.join("original.png"), b"replaced").is_err());
    assert!(fs::write(stage_root.join("transfer.json"), b"replaced").is_err());
    assert_eq!(sink.verified_bytes().unwrap(), bytes);
    sink.result_sealed().unwrap();
    assert_eq!(fs::read_dir(&stage_root).unwrap().count(), 0);
    drop(sink);
    fs::rename(&stage_root, root.path().join("moved")).unwrap();
}

#[cfg(unix)]
#[test]
fn substituted_staging_path_is_never_deleted_as_the_original() {
    let root = tempfile::tempdir().unwrap();
    let (request, _, transfer) = fixture::request();
    let bytes = fixture::png();
    let header = fixture::header(&transfer, &bytes);
    let mut sink = DesktopOutputAssembler::create(root.path(), transfer, request.clone()).unwrap();
    sink.begin(header.clone()).unwrap();
    sink.write_chunk(fixture::chunk(&header, &bytes, 0), &bytes)
        .unwrap();
    sink.finish(fixture::finish(&request, &header)).unwrap();
    fs::rename(
        root.path().join("original.png"),
        root.path().join("retained.png"),
    )
    .unwrap();
    fs::write(root.path().join("original.png"), b"replacement").unwrap();
    assert!(sink.result_sealed().is_err());
    assert_eq!(
        fs::read(root.path().join("original.png")).unwrap(),
        b"replacement"
    );
    assert_eq!(fs::read(root.path().join("retained.png")).unwrap(), bytes);
}
