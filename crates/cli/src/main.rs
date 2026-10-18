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
    let data_arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if data_arguments.first().is_some_and(|s| s == "data") {
        let operation=data_arguments.get(1).ok_or_else(||"usage: forme-cli data <backup|verify-backup|restore-plan|restore|lifecycle> [JSON; defaults to stdin]".to_owned())?;
        let json = if data_arguments.len() > 2 {
            data_arguments[2..].join(" ")
        } else {
            let mut body = String::new();
            std::io::stdin()
                .take(1024 * 1024)
                .read_to_string(&mut body)
                .map_err(|_| "data command could not be read".to_owned())?;
            body
        };
        let request = serde_json::from_str(&json)
            .map_err(|_| "data command must be valid JSON".to_owned())?;
        let response = forme_gateway::HttpGatewayClient::from_environment()
            .and_then(|gateway| gateway.data_control(operation, &request))
            .map_err(|e| e.to_string())?;
        println!(
            "{}",
            serde_json::to_string_pretty(&response)
                .map_err(|_| "data response encoding failed".to_owned())?
        );
        return Ok(());
    }
    if data_arguments.first().is_some_and(|s| s == "brain")
        && data_arguments.get(1).is_some_and(|s| s == "dashboard")
    {
        let response = forme_gateway::HttpGatewayClient::from_environment()
            .and_then(|gateway| gateway.brain_dashboard(data_arguments.get(2).map(String::as_str)))
            .map_err(|e| e.to_string())?;
        println!(
            "{}",
            serde_json::to_string_pretty(&response)
                .map_err(|_| "dashboard encoding failed".to_owned())?
        );
        return Ok(());
    }
    let arguments = parse_arguments()?;
    // The default path talks to the running local gateway daemon, which owns
    // the single production runtime. `--embedded` composes that same runtime
    // in-process through the same builder, for use when no daemon is running.
    let gateway: Box<dyn forme_gateway::RunGateway> = if arguments.embedded {
        forme_gateway::environment_gateway().map_err(|error| error.to_string())?
    } else {
        Box::new(
            forme_gateway::HttpGatewayClient::from_environment()
                .map_err(|error| error.to_string())?,
        )
    };
    if arguments.manifest {
        let digest = gateway
            .runtime_manifest_digest()
            .map_err(|error| error.to_string())?;
        println!("{}", digest.0);
        return Ok(());
    }
    let (session, question) = (arguments.session, arguments.question);
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

struct Arguments {
    session: String,
    question: String,
    embedded: bool,
    manifest: bool,
}

fn parse_arguments() -> Result<Arguments, String> {
    let mut args = std::env::args().skip(1).peekable();
    let mut session = "cli:default".to_owned();
    let mut embedded = false;
    let mut manifest = false;
    let mut question = Vec::new();
    while let Some(argument) = args.next() {
        if argument == "--session" {
            session = args
                .next()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "--session requires a value".to_owned())?;
        } else if argument == "--embedded" {
            embedded = true;
        } else if argument == "--manifest" {
            manifest = true;
        } else {
            question.push(argument);
        }
    }
    if manifest {
        if !question.is_empty() {
            return Err("--manifest does not take a question".into());
        }
        return Ok(Arguments {
            session,
            question: String::new(),
            embedded,
            manifest,
        });
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
    Ok(Arguments {
        session,
        question,
        embedded,
        manifest,
    })
}
