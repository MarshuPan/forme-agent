//! Snapshot capture and folding for the two builtin projection caches.
use super::*;

pub(super) struct BuiltinRebuildSnapshot {
    pub(super) events: Vec<StoredEvent>,
    pub(super) token: EventSetToken,
    pub(super) watermarks: BTreeMap<String, u64>,
}

pub(super) fn capture_rebuild_events(
    connection: &mut Connection,
) -> p::Result<BuiltinRebuildSnapshot> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(|error| store_error("failed to begin projection rebuild snapshot", error))?;
    let events = read_all_stored_events(&transaction)?;
    let token = EventSetToken::from_stored_events(&events);
    let watermarks = crate::projection_privacy::metadata_watermarks(&transaction, None)?;
    transaction
        .commit()
        .map_err(|error| store_error("failed to finish projection rebuild snapshot", error))?;
    Ok(BuiltinRebuildSnapshot {
        events,
        token,
        watermarks,
    })
}

pub(super) fn fold_builtin_projections(
    stored_events: Vec<StoredEvent>,
    current_schema: &BTreeMap<PayloadType, p::SchemaVersion>,
    upcasters: &UpcasterGraph,
    watermarks: &BTreeMap<String, u64>,
) -> p::Result<Vec<BuiltinProjectionState>> {
    let mut streams = BTreeMap::<String, Vec<StoredEvent>>::new();
    for event in stored_events {
        streams.entry(event.run_id.clone()).or_default().push(event);
    }
    for run in watermarks.keys() {
        streams.entry(run.clone()).or_default();
    }

    let mut rebuilt = Vec::with_capacity(streams.len());
    for (run, mut stream) in streams {
        stream.sort_by_key(|event| event.stream_seq);
        let mut session = SessionStateProjection::empty();
        let mut transcript = TranscriptProjection::empty();
        let mut fts_entries = Vec::new();
        for stored_event in stream {
            let event = stored_event.decode(current_schema, upcasters)?;
            if let Some(text) = searchable_text(&event) {
                fts_entries.push(FtsEntry {
                    event_id: event.event_id.0.clone(),
                    text: text.to_owned(),
                });
            }
            SessionStateProjection::apply(&mut session, &event);
            TranscriptProjection::apply(&mut transcript, &event);
        }
        if let Some(head) = watermarks.get(&run) {
            crate::projection_privacy::restore_watermark(
                &run,
                *head,
                &mut session,
                &mut transcript,
            );
        }
        rebuilt.push(BuiltinProjectionState {
            session,
            transcript,
            fts_entries,
        });
    }
    Ok(rebuilt)
}
