#[cfg(not(debug_assertions))]
compile_error!(
    "mahayana-test-driver is forbidden in release builds; use a Debug/test-signed build"
);

#[cfg(not(debug_assertions))]
fn main() {}

#[cfg(debug_assertions)]
#[path = "mahayana-test-driver/backend.rs"]
mod backend;

#[cfg(debug_assertions)]
use backend::ProductBackend;
#[cfg(debug_assertions)]
use mahayana_test_driver_protocol::{TestDriverBackend, TestDriverSession};
#[cfg(debug_assertions)]
use std::io::{self, BufRead, Write};

#[cfg(debug_assertions)]
fn main() {
    if let Err(error) = run_stdio() {
        eprintln!("mahayana-test-driver: {error}");
        std::process::exit(1);
    }
}

#[cfg(debug_assertions)]
fn run_stdio() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let backend = ProductBackend::from_environment().map_err(|error| error.to_string())?;
    let mut session = TestDriverSession::new(backend);

    for line in stdin.lock().lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let response = session.execute_json_line(&line);
        writeln!(stdout, "{response}").map_err(|error| error.to_string())?;
        stdout.flush().map_err(|error| error.to_string())?;
        if session.shutdown_requested() {
            break;
        }
    }
    Ok(())
}
