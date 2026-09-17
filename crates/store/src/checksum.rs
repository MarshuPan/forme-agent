//! Deterministic corruption checksum, not a cryptographic authenticity mechanism.

const FNV_OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
const FNV_PRIME: u64 = 1_099_511_628_211;

pub(crate) struct PersistedEnvelope<'a> {
    pub event_id: &'a str,
    pub run_id: &'a str,
    pub stream_seq: u64,
    pub turn_id: Option<&'a str>,
    pub kind: &'a str,
    pub payload: &'a [u8],
    pub schema_version: u32,
    pub ts_unix_ms: i64,
    pub provenance: &'a [u8],
}

pub(crate) fn calculate(envelope: PersistedEnvelope<'_>) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    write_sized(&mut hash, envelope.event_id.as_bytes());
    write_sized(&mut hash, envelope.run_id.as_bytes());
    write(&mut hash, &envelope.stream_seq.to_le_bytes());
    match envelope.turn_id {
        Some(turn_id) => {
            write(&mut hash, &[1]);
            write_sized(&mut hash, turn_id.as_bytes());
        }
        None => write(&mut hash, &[0]),
    }
    write_sized(&mut hash, envelope.kind.as_bytes());
    write_sized(&mut hash, envelope.payload);
    write(&mut hash, &envelope.schema_version.to_le_bytes());
    write(&mut hash, &envelope.ts_unix_ms.to_le_bytes());
    write_sized(&mut hash, envelope.provenance);
    format!("{hash:016x}")
}

pub(crate) fn calculate_bytes(bytes: &[u8]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    write_sized(&mut hash, b"forme-redacted-payload-v1");
    write_sized(&mut hash, bytes);
    format!("{hash:016x}")
}

fn write_sized(hash: &mut u64, bytes: &[u8]) {
    write(hash, &(bytes.len() as u64).to_le_bytes());
    write(hash, bytes);
}

fn write(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}
