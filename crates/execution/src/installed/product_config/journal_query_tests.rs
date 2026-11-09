//! Fault injection through the actual locked handle; OS locking stays intact.
use super::super::{
    tests::{fixture, patch},
    ProductConfigStore,
};
use super::*;

fn bytes(store: &mut ProductConfigStore) -> Vec<u8> {
    let file = &mut store.journal.file;
    let cursor = file.stream_position().unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut result = Vec::new();
    file.read_to_end(&mut result).unwrap();
    file.seek(SeekFrom::Start(cursor)).unwrap();
    result
}

#[test]
fn desktop_settings_query_rechecks_the_original_journal_and_never_recreates_missing_facts() {
    let (_root, _path, store) = fixture();
    let mut store = store.into_inner().unwrap();
    store.apply(patch("save:base", 0)).unwrap();
    // Production uses an append-only handle, which cannot truncate on Windows.
    // Fault injection retains its projection and acquires the same exclusive lock
    // with a writable test handle; production access rights remain unchanged.
    let ConfigJournal {
        file,
        projection,
        poisoned,
    } = store.journal;
    drop(file);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(
            store
                .template
                .core_config_path
                .with_extension("journal.jsonl"),
        )
        .unwrap();
    file.try_lock().unwrap();
    store.journal = ConfigJournal {
        file,
        projection,
        poisoned,
    };
    let original = bytes(&mut store);
    assert!(
        store
            .computer_settings_operation("save:base")
            .unwrap()
            .committed
    );
    assert_eq!(bytes(&mut store), original);
    let last_line = original[..original.len() - 1]
        .iter()
        .rposition(|b| *b == b'\n')
        .unwrap()
        + 1;
    store.journal.file.set_len(last_line as u64).unwrap();
    store.journal.file.sync_all().unwrap();
    assert!(store.computer_settings_operation("save:base").is_err());
    assert_eq!(bytes(&mut store), original[..last_line]);
    store.journal.file.set_len(0).unwrap();
    store.journal.file.seek(SeekFrom::Start(0)).unwrap();
    store.journal.file.write_all(&original).unwrap();
    store.journal.file.sync_all().unwrap();
    // Restoring bytes cannot silently clear a writer that observed corruption.
    assert!(store.computer_settings_operation("save:base").is_err());
    assert!(store.apply(patch("save:after-corruption", 1)).is_err());
}
