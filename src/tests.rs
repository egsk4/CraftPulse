#[cfg(target_os = "linux")]
#[test]
fn fake_discord_socket_receives_presence_and_clear_commands() {
    use crate::presence::{PresenceCard, RpcWorker};
    use serde_json::{json, Value};
    use std::{
        fs,
        io::{Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        sync::Mutex,
        thread,
        time::Duration,
    };

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp = std::env::temp_dir().join(format!("craftpulse-fake-discord-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).unwrap();
    let socket_path = temp.join("discord-ipc-0");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let previous_runtime = std::env::var_os("XDG_RUNTIME_DIR");
    std::env::set_var("XDG_RUNTIME_DIR", &temp);

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let (handshake_opcode, handshake) = read_packet(&mut stream);
        assert_eq!(handshake_opcode, 0);
        assert_eq!(handshake["client_id"], "1558510318513033329");
        write_packet(&mut stream, 1, &json!({"v":1,"user":{"id":"fake"}}));

        let (activity_opcode, activity) = read_packet(&mut stream);
        let (clear_opcode, clear) = read_packet(&mut stream);
        let (close_opcode, _) = read_packet(&mut stream);
        (activity_opcode, activity, clear_opcode, clear, close_opcode)
    });

    let worker = RpcWorker::new();
    let card = PresenceCard {
        app_name: "PhotoCraft".to_string(),
        document: "Poster".to_string(),
        state: "in PhotoCraft — Editing".to_string(),
        large_image_key: "photocraft".to_string(),
        small_image_key: "artcraft".to_string(),
    };
    assert_eq!(worker.set_activity(card), (true, false));
    worker.clear();
    worker.quit();
    let (activity_opcode, activity, clear_opcode, clear, close_opcode) =
        server.join().expect("fake Discord server should finish");
    assert_eq!(activity_opcode, 1);
    assert_eq!(activity["cmd"], "SET_ACTIVITY");
    assert_eq!(activity["args"]["activity"]["details"], "Poster");
    assert_eq!(
        activity["args"]["activity"]["state"],
        "in PhotoCraft — Editing"
    );
    assert_eq!(
        activity["args"]["activity"]["assets"]["large_image"],
        "photocraft"
    );
    assert_eq!(
        activity["args"]["activity"]["assets"]["small_image"],
        "artcraft"
    );
    assert_eq!(clear_opcode, 1);
    assert_eq!(clear["args"]["activity"], Value::Null);
    assert_eq!(close_opcode, 2);

    if let Some(previous) = previous_runtime {
        std::env::set_var("XDG_RUNTIME_DIR", previous);
    } else {
        std::env::remove_var("XDG_RUNTIME_DIR");
    }
    let _ = fs::remove_dir_all(temp);

    fn read_packet(stream: &mut UnixStream) -> (u32, Value) {
        let mut header = [0_u8; 8];
        stream.read_exact(&mut header).unwrap();
        let opcode = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        (opcode, serde_json::from_slice(&body).unwrap())
    }

    fn write_packet(stream: &mut UnixStream, opcode: u32, value: &Value) {
        let body = value.to_string();
        stream.write_all(&opcode.to_le_bytes()).unwrap();
        stream
            .write_all(&(body.len() as u32).to_le_bytes())
            .unwrap();
        stream.write_all(body.as_bytes()).unwrap();
    }
}
