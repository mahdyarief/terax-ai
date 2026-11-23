//! `terax-cli` as OpenSSH's `SSH_ASKPASS` program. Terax starts `ssh` with
//! `TERAX_SSH_ASKPASS=1`, its control endpoint and a token for this one
//! connection; `ssh` runs this with its prompt as the argument. The answer
//! goes to stdout, the only place `ssh` reads it from; it is never logged.
//! A dismissed dialog, or any failure, exits 1: `ssh` reads that as no
//! answer and stops asking.

use std::ffi::OsString;
use std::io::{BufReader, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::time::Duration;

use terax_control_protocol::askpass::{
    AskpassAnswer, AskpassParams, ASKPASS_WAIT, CANCELLED, METHOD_ASKPASS,
};
use terax_control_protocol::{
    CallerContext, ControlDescriptor, ControlRequest, ControlResponse, PROTOCOL_VERSION,
};

use crate::{
    io_error, parse_loopback_address, read_response, request_id, validate_endpoint, write_request,
    CliError,
};

const CONNECT: Duration = Duration::from_secs(2);
/// Past the user's time, so the app's own timeout answer arrives first.
const SLACK: Duration = Duration::from_secs(15);

pub fn main(args: &[OsString]) -> ExitCode {
    let code = run(
        args,
        |name| std::env::var(name).ok(),
        send,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}

pub(crate) fn run(
    args: &[OsString],
    lookup: impl Fn(&str) -> Option<String>,
    send: impl FnOnce(&str, &ControlRequest) -> Result<ControlResponse, CliError>,
    out: &mut impl Write,
    err: &mut impl Write,
) -> u8 {
    match ask(args, &lookup, send) {
        Ok(Reply::Text(answer)) => {
            let written = out
                .write_all(answer.as_bytes())
                .and_then(|()| out.write_all(b"\n"))
                .and_then(|()| out.flush());
            u8::from(written.is_err())
        }
        Ok(Reply::Exit(code)) => code,
        Err(error) => {
            if error.code != CANCELLED {
                let _ = writeln!(err, "terax askpass: {}", error.message);
            }
            1
        }
    }
}

enum Reply {
    Text(String),
    Exit(u8),
}

fn ask(
    args: &[OsString],
    lookup: &impl Fn(&str) -> Option<String>,
    send: impl FnOnce(&str, &ControlRequest) -> Result<ControlResponse, CliError>,
) -> Result<Reply, CliError> {
    // Only the endpoint Terax gave this `ssh`; never the descriptor file,
    // which would let any `ssh` ask through whichever Terax is running.
    let (Some(address), Some(token)) =
        (lookup("TERAX_CONTROL_ADDR"), lookup("TERAX_CONTROL_TOKEN"))
    else {
        return Err(CliError::new(
            "invalid_environment",
            "TERAX_CONTROL_ADDR and TERAX_CONTROL_TOKEN are not set",
            1,
        ));
    };
    let endpoint = validate_endpoint(
        ControlDescriptor {
            protocol: PROTOCOL_VERSION,
            address,
            token,
            pid: 0,
            app_version: String::new(),
        },
        false,
    )?;
    let prompt = args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let hint = lookup("SSH_ASKPASS_PROMPT").filter(|hint| !hint.is_empty());
    let confirm = hint.as_deref() == Some("confirm");
    let notice = hint.as_deref() == Some("none");
    let params = AskpassParams { prompt, hint };
    let request = ControlRequest {
        protocol: PROTOCOL_VERSION,
        id: request_id(),
        token: endpoint.token,
        method: METHOD_ASKPASS.to_string(),
        params: serde_json::to_value(params).expect("askpass params serialize"),
        caller: CallerContext::default(),
    };
    let response = send(&endpoint.address, &request)?;
    if !response.ok {
        let error = response.error.unwrap_or_else(|| {
            terax_control_protocol::ControlError::new(
                "request_failed",
                "Terax refused the question",
            )
        });
        return Err(CliError::new(error.code, error.message, 1));
    }
    let answer: AskpassAnswer = serde_json::from_value(response.result.unwrap_or_default())
        .map_err(|_| CliError::new("invalid_response", "Terax returned no answer", 1))?;
    if confirm {
        return Ok(Reply::Exit(u8::from(!answer.confirmed)));
    }
    if notice {
        return Ok(Reply::Exit(0));
    }
    Ok(Reply::Text(answer.answer))
}

fn send(address: &str, request: &ControlRequest) -> Result<ControlResponse, CliError> {
    let address = parse_loopback_address(address)?;
    let mut stream = TcpStream::connect_timeout(&address, CONNECT).map_err(io_error)?;
    stream.set_write_timeout(Some(CONNECT)).ok();
    stream.set_read_timeout(Some(ASKPASS_WAIT + SLACK)).ok();
    write_request(&mut stream, request)?;
    read_response(&mut BufReader::new(stream), request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn env(extra: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        // The extras come first, so they override the defaults.
        let mut vars: Vec<(String, String)> = extra
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        vars.push(("TERAX_CONTROL_ADDR".into(), "127.0.0.1:4242".into()));
        vars.push(("TERAX_CONTROL_TOKEN".into(), TOKEN.into()));
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    struct Outcome {
        code: u8,
        out: String,
        err: String,
        sent: Option<ControlRequest>,
    }

    fn ask_with(
        args: &[&str],
        lookup: impl Fn(&str) -> Option<String>,
        reply: Result<ControlResponse, CliError>,
    ) -> Outcome {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let sent = RefCell::new(None);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(
            &args,
            lookup,
            |address, request| {
                assert_eq!(address, "127.0.0.1:4242");
                *sent.borrow_mut() = Some(request.clone());
                reply
            },
            &mut out,
            &mut err,
        );
        Outcome {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            sent: sent.into_inner(),
        }
    }

    fn answered(answer: &str, confirmed: bool) -> Result<ControlResponse, CliError> {
        Ok(ControlResponse::success(
            "x",
            json!({ "answer": answer, "confirmed": confirmed }),
        ))
    }

    #[test]
    fn the_prompt_goes_to_terax_and_the_answer_to_stdout() {
        let run = ask_with(
            &["me@dypod's password: "],
            env(&[]),
            answered("hunter2", false),
        );
        assert_eq!(
            (run.code, run.out.as_str(), run.err.as_str()),
            (0, "hunter2\n", "")
        );
        let sent = run.sent.unwrap();
        assert_eq!(sent.method, METHOD_ASKPASS);
        assert_eq!(sent.token, TOKEN);
        assert_eq!(sent.params, json!({ "prompt": "me@dypod's password: " }));
        assert_eq!(sent.caller, CallerContext::default());
    }

    #[test]
    fn a_cancelled_dialog_answers_nothing() {
        let run = ask_with(
            &["me@dypod's password: "],
            env(&[]),
            Ok(ControlResponse::failure("x", CANCELLED, "dismissed")),
        );
        assert_eq!((run.code, run.out.as_str(), run.err.as_str()), (1, "", ""));
    }

    #[test]
    fn failures_answer_nothing_and_say_why() {
        let run = ask_with(
            &["p"],
            env(&[]),
            Ok(ControlResponse::failure(
                "x",
                "frontend_timeout",
                "Terax UI did not respond in time",
            )),
        );
        assert_eq!((run.code, run.out.as_str()), (1, ""));
        assert!(run.err.contains("did not respond"), "{}", run.err);
        let run = ask_with(
            &["p"],
            env(&[]),
            Err(CliError::new(
                "app_unavailable",
                "could not connect to Terax",
                3,
            )),
        );
        assert_eq!((run.code, run.out.as_str()), (1, ""));
    }

    #[test]
    fn without_its_endpoint_it_asks_nobody() {
        let run = ask_with(&["p"], |_| None, answered("x", false));
        assert_eq!((run.code, run.out.as_str()), (1, ""));
        assert!(run.sent.is_none());
        let run = ask_with(
            &["p"],
            env(&[("TERAX_CONTROL_TOKEN", "short")]),
            answered("x", false),
        );
        assert!(run.sent.is_none());
        assert_eq!(run.code, 1);
    }

    #[test]
    fn confirmations_answer_by_exit_status_and_notices_succeed() {
        let confirm = env(&[("SSH_ASKPASS_PROMPT", "confirm")]);
        let allowed = ask_with(&["Allow use of key?"], &confirm, answered("", true));
        assert_eq!((allowed.code, allowed.out.as_str()), (0, ""));
        assert_eq!(
            allowed.sent.unwrap().params,
            json!({ "prompt": "Allow use of key?", "hint": "confirm" })
        );
        let denied = ask_with(&["Allow use of key?"], &confirm, answered("", false));
        assert_eq!((denied.code, denied.out.as_str()), (1, ""));
        let notice = ask_with(
            &["Confirm user presence"],
            env(&[("SSH_ASKPASS_PROMPT", "none")]),
            answered("", false),
        );
        assert_eq!((notice.code, notice.out.as_str()), (0, ""));
    }

    #[test]
    fn the_answer_never_reaches_stderr() {
        let run = ask_with(
            &["p"],
            env(&[]),
            Ok(ControlResponse::success("x", json!({}))),
        );
        assert_eq!((run.code, run.out.as_str()), (0, "\n"));
        let run = ask_with(
            &["p"],
            env(&[]),
            Ok(ControlResponse::success("x", json!("hunter2"))),
        );
        assert_eq!(run.code, 1);
        assert!(!run.err.contains("hunter2"), "{}", run.err);
    }
}
