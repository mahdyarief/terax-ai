//! Answers to ssh's questions (passwords, passphrases, OTPs) pass from the
//! dialog through the control server to `terax-cli`'s stdout and nowhere
//! else. This reads the sources on that path and fails if a logging call,
//! or a line pushed to a connection's log, formats anything that could hold
//! an answer.

use std::path::{Path, PathBuf};

/// Calls that write somewhere a person or a file keeps.
const SINKS: [&str; 8] = [
    "log::",
    "tracing::",
    "println!",
    "eprintln!",
    "dbg!",
    "writeln!(err",
    "log.push(",
    "emit(",
];

/// What an answer travels in.
const CARRIERS: [&str; 7] = [
    "answer", "response", "result", "secret", "frontend", ":?}", "{:#?}",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn on_the_path() -> Vec<PathBuf> {
    let root = root();
    let mut files = vec![
        root.join("src/modules/control.rs"),
        root.join("crates/terax-cli/src/askpass.rs"),
        root.join("crates/terax-control-protocol/src/askpass.rs"),
    ];
    for entry in std::fs::read_dir(root.join("src/modules/remote")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".rs") && !name.ends_with("_tests.rs") {
            files.push(path);
        }
    }
    files
}

/// The source before its inline test module, without line comments.
fn production(source: &str) -> String {
    let mut kept = String::new();
    let lines: Vec<&str> = source.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let next = lines.get(i + 1).map(|l| l.trim()).unwrap_or("");
        if trimmed.starts_with("#[cfg(") && trimmed.contains("test") && next.starts_with("mod ") {
            break;
        }
        if !trimmed.starts_with("//") {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept
}

/// Each sink call in `source`, up to the end of its statement.
fn sink_calls(source: &str) -> Vec<String> {
    let mut calls = Vec::new();
    for sink in SINKS {
        let mut from = 0;
        while let Some(at) = source[from..].find(sink) {
            let start = from + at;
            let end = source[start..]
                .find(';')
                .map_or(source.len(), |end| start + end);
            calls.push(source[start..end].to_string());
            from = start + sink.len();
        }
    }
    calls
}

fn leaks(call: &str) -> bool {
    let lower = call.to_ascii_lowercase();
    CARRIERS.iter().any(|carrier| lower.contains(carrier))
}

/// Sinks that are allowed to mention a carrier word, with why.
const ALLOWED: &[(&str, &str)] = &[(
    "emit(",
    "control.rs hands requests (questions, not answers) to the window",
)];

#[test]
fn no_log_on_the_askpass_path_formats_an_answer() {
    let mut problems = Vec::new();
    for path in on_the_path() {
        let source = std::fs::read_to_string(&path).unwrap();
        for call in sink_calls(&production(&source)) {
            let allowed = ALLOWED.iter().any(|(start, _)| call.starts_with(start));
            if leaks(&call) && !allowed {
                problems.push(format!("{}: {}", path.display(), call.trim()));
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn the_scan_sees_what_it_guards() {
    let source = "fn a() {\n    log::info!(\"got {}\", answer.answer);\n    log.push(format!(\"{response:?}\"));\n    log.push(\"connected\");\n}\n\
                  #[cfg(test)]\nmod tests {\n    fn t() { println!(\"{answer}\"); }\n}\n";
    let calls = sink_calls(&production(source));
    let leaking: Vec<_> = calls.iter().filter(|c| leaks(c)).collect();
    assert_eq!(leaking.len(), 2, "{calls:?}");
    assert!(calls.iter().any(|c| c.contains("\"connected\"")));
    assert!(
        !calls.iter().any(|c| c.contains("println")),
        "tests are skipped"
    );
}
