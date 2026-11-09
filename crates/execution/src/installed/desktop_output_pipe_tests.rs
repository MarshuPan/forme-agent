//! Actual nonblocking Windows pipes. No installed service or desktop is touched.
use super::*;
use crate::installed::windows_desktop_helper_peer_native::{
    create_pipe, open_output_pipe, prepare_output_peer,
};
use std::sync::atomic::{AtomicU64, Ordering};

fn pair() -> (File, File) {
    let output = std::process::Command::new("whoami.exe")
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let sid = text
        .trim()
        .split(',')
        .next_back()
        .unwrap()
        .trim_matches('"');
    assert!(sid.starts_with("S-1-"));
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let name = format!(
        r"\\.\pipe\forme-desktop-helper-v1-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let (server, _security) = create_pipe(&name, sid).unwrap();
    let client = open_output_pipe(&name, Instant::now() + Duration::from_secs(2)).unwrap();
    // OwnedHandle -> File is a safe ownership transfer. The private test pipe
    // is already connected by the client's successful CreateFile call.
    (File::from(server), client)
}

#[test]
fn desktop_output_actual_pipe_preserves_control_and_raw_bytes_in_both_directions() {
    let (mut server, mut client) = pair();
    for reverse in [false, true] {
        let (writer, reader) = if reverse {
            (&mut client, &mut server)
        } else {
            (&mut server, &mut client)
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        let bytes = [0, 255, 137, 1, 10, 128];
        write_output_frame(writer, &vec!["self-authored", "control"], deadline).unwrap();
        write_output_bytes(writer, &bytes, deadline).unwrap();
        let frame: Vec<String> = read_frame(reader, deadline).unwrap();
        assert_eq!(frame, ["self-authored", "control"]);
        assert_eq!(
            read_output_bytes(reader, bytes.len() as u32, deadline).unwrap(),
            bytes
        );
    }
}

#[test]
fn desktop_output_actual_pipe_stalled_writes_and_reads_keep_the_shared_deadline() {
    for client_writes in [false, true] {
        let (mut server, mut client) = pair();
        let writer = if client_writes {
            &mut client
        } else {
            &mut server
        };
        let began = Instant::now();
        let deadline = began + Duration::from_millis(150);
        let block = vec![61; p::DESKTOP_OUTPUT_CHUNK_BYTES as usize];
        let mut stopped = false;
        for _ in 0..512 {
            if write_output_bytes(writer, &block, deadline).is_err() {
                stopped = true;
                break;
            }
        }
        assert!(stopped, "a non-reading peer must exhaust the finite pipe");
        assert!(began.elapsed() < Duration::from_secs(3));
        assert!(write_output_frame(writer, &"finish", deadline).is_err());
    }
    let (mut server, _client) = pair();
    let began = Instant::now();
    assert!(read_output_bytes(&mut server, 1, began + Duration::from_millis(50)).is_err());
    assert!(began.elapsed() < Duration::from_secs(3));
}

#[test]
fn desktop_output_actual_pipe_refuses_foreign_host_and_disconnected_peer() {
    let (mut server, client) = pair();
    // A genuine local pipe owned by this test process is still not the SCM Host.
    assert!(prepare_output_peer(&client).is_err());
    drop(client);
    let began = Instant::now();
    assert!(write_output_bytes(&mut server, &[1, 2], began + Duration::from_millis(100)).is_err());
    assert!(read_output_bytes(&mut server, 1, began + Duration::from_millis(100)).is_err());
    assert!(began.elapsed() < Duration::from_secs(3));
}
