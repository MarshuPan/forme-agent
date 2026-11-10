#![forbid(unsafe_code)]
//! Original finite protocol peer for native isolation tests, never installed.
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, Write},
    time::Duration,
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("normal");
    let mut trace = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open("requests.jsonl")
        .unwrap();
    fs::write("started.pid", std::process::id().to_string()).unwrap();
    if mode == "no-read" {
        std::thread::sleep(Duration::from_secs(20));
        return;
    }
    let mut input = io::stdin().lock();
    let mut tool_page = 0;
    let mut methods = vec![];
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).unwrap() == 0 {
            break;
        }
        assert!(line.len() <= 1_048_576);
        trace.write_all(line.as_bytes()).unwrap();
        trace.sync_all().unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        let method = request["method"].as_str().unwrap();
        methods.push(method.to_owned());
        let id = request.get("id").cloned();
        if method == "notifications/initialized" {
            assert!(id.is_none());
            if mode == "call-no-read" {
                while !std::path::Path::new("continue").exists() {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            continue;
        }
        if mode == "hang" {
            std::thread::sleep(Duration::from_secs(20));
            return;
        }
        if mode == "hold" && method == "initialize" {
            while !std::path::Path::new("continue").exists() {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        if mode == "stderr" {
            io::stderr().write_all(&vec![b'e'; 131_000]).unwrap();
        }
        if mode == "oversized" {
            io::stdout().write_all(&vec![b'x'; 131_073]).unwrap();
            return;
        }
        if mode == "malformed" {
            println!("not json");
            io::stdout().flush().unwrap();
            continue;
        }
        let result = match method {
            "initialize" => {
                json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{},"resources":{}},
                "serverInfo":{"name":"forme-native-fixture","version":"1"}})
            }
            "tools/list" => {
                tool_page += 1;
                if tool_page == 1 {
                    assert!(request["params"].get("cursor").is_none());
                    json!({"tools":[{"name":"fixture.inspect","description":probe(&args).to_string(),
                        "inputSchema":{"type":"object","properties":{}}}],"nextCursor":"fixture:second"})
                } else {
                    assert_eq!(request["params"]["cursor"], "fixture:second");
                    json!({"tools":[]})
                }
            }
            "resources/list" => json!({"resources":[{"uri":"fixture://methods","name":"trace",
                "description":serde_json::to_string(&methods).unwrap()}]}),
            "tools/call" => {
                assert_eq!(request["params"]["name"], "fixture.inspect");
                fs::write("effect.json", request["params"].to_string()).unwrap();
                if mode == "call-hang" {
                    std::thread::sleep(Duration::from_secs(20));
                    return;
                }
                if mode == "call-held-result" {
                    while !std::path::Path::new("continue").exists() {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                if mode == "call-malformed" {
                    println!("untrusted invalid call output");
                    io::stdout().flush().unwrap();
                    continue;
                }
                if mode == "call-error" {
                    println!(
                        "{}",
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"fixture rejection"}})
                    );
                    io::stdout().flush().unwrap();
                    continue;
                }
                json!({"content":[{"type":"text","text":serde_json::to_string(&methods).unwrap()}],
                    "structuredContent":{"arguments":request["params"]["arguments"]},
                    "isError":mode == "call-tool-error"})
            }
            _ => panic!("unexpected method"),
        };
        let id = if mode == "wrong-id" {
            json!(99)
        } else {
            id.unwrap()
        };
        let response = json!({"jsonrpc":"2.0","id":id,"result":result});
        if mode == "incomplete" {
            print!("{response}");
            io::stdout().flush().unwrap();
            return;
        }
        println!("{response}");
        if mode == "duplicate" {
            println!("{response}");
        }
        io::stdout().flush().unwrap();
    }
    if mode == "tail"
        || (mode == "call-tail" && methods.iter().any(|method| method == "tools/call"))
    {
        println!("{{}}");
    }
    if mode == "nonzero"
        || (mode == "call-nonzero" && methods.iter().any(|method| method == "tools/call"))
    {
        std::process::exit(7);
    }
}
fn probe(args: &[String]) -> Value {
    let own = std::env::current_exe().unwrap();
    let cwd = std::env::current_dir().unwrap();
    let child_denied = std::process::Command::new(&own)
        .arg("no-read")
        .spawn()
        .is_err();
    let sources_denied = fs::File::open(own.parent().unwrap().join("sources.json")).is_err();
    let records_denied =
        fs::File::open(cwd.parent().unwrap().join("result-seal-pending.json")).is_err();
    let private_denied = args.get(2).is_none_or(|path| {
        fs::File::open(path).is_err() && fs::OpenOptions::new().write(true).open(path).is_err()
    });
    let network_denied = args.get(3).is_none_or(|address| {
        std::net::TcpStream::connect_timeout(&address.parse().unwrap(), Duration::from_millis(300))
            .is_err()
    });
    let environment_closed = std::env::vars_os().all(|(key, _)| {
        key.to_str().is_some_and(|key| {
            ["LOCALAPPDATA", "SYSTEMROOT", "TEMP", "TMP", "WINDIR"]
                .contains(&key.to_ascii_uppercase().as_str())
        })
    });
    json!({"child_denied":child_denied,"sources_denied":sources_denied,
        "records_denied":records_denied,"private_denied":private_denied,
        "network_denied":network_denied,"environment_closed":environment_closed})
}
