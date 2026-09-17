#![forbid(unsafe_code)]

use std::io::Read;

use forme_cli::{CliClient, CliRequest};
use forme_protocol as p;

fn main() {
    if let Err(error) = run() {
        eprintln!("forme: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (session, question) = parse_arguments()?;
    let gateway = forme_gateway::environment_gateway().map_err(|error| error.to_string())?;
    let output = CliClient::new(gateway.as_ref())
        .ask(CliRequest {
            schema_version: p::SchemaVersion(1),
            session: p::SessionRef(session),
            question,
        })
        .map_err(|error| error.to_string())?;
    println!("{}", output.answer);
    Ok(())
}

fn parse_arguments() -> Result<(String, String), String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut session = "cli:default".to_owned();
    let mut question = Vec::new();
    while let Some(argument) = args.next() {
        if argument == "--session" {
            session = args
                .next()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "--session requires a value".to_owned())?;
        } else {
            question.push(argument);
        }
    }
    let question = if question.is_empty() {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|error| format!("failed to read question from stdin: {error}"))?;
        input
    } else {
        question.join(" ")
    };
    if question.trim().is_empty() {
        return Err("provide a question as arguments or stdin".into());
    }
    Ok((session, question))
}
