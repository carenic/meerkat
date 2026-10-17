//! Operator-facing rendering of a fatal error in a runtime-backed binary.
//!
//! `fn main() -> Result<(), Box<dyn Error>>` makes Rust print a returned error
//! with `Debug`, so a storage refusal at startup read
//! `Error: Store(UnledgeredDomainObjects { domain: "session-store", .. })` and
//! the remedy sentence that only the `Display` form carries was never shown.
//! The binaries (`rkat-rpc`, `rkat-rest`, `rkat-mcp`, `rkat-mob-mcp`) render
//! through this module instead: the `Display` chain (the error, then each
//! `source()`), every line prefixed with the binary name, and exit status 1.
//! When an [`std::io::Error`] of kind `WouldBlock` (EAGAIN) is in the chain,
//! the OS refused a thread or process, and a last line says what usually
//! causes that and which knob lowers the worker count
//! ([`meerkat_runtime::host_stack::thread_limit_hint`]).

use std::error::Error;
use std::fmt;
use std::process::ExitCode;

/// `Display` chain of a fatal error, one line per link, each prefixed with
/// `<binary>: `. Sources follow the head error as `caused by:` lines, and a
/// refused thread or process spawn adds a `hint:` line.
pub struct FatalErrorReport<'a> {
    binary: &'a str,
    error: &'a (dyn Error + 'static),
}

impl<'a> FatalErrorReport<'a> {
    pub fn new(binary: &'a str, error: &'a (dyn Error + 'static)) -> Self {
        Self { binary, error }
    }
}

impl fmt::Display for FatalErrorReport<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.binary, self.error)?;
        let mut source = self.error.source();
        while let Some(cause) = source {
            write!(f, "\n{}: caused by: {cause}", self.binary)?;
            source = cause.source();
        }
        let chain = std::iter::successors(Some(self.error), |&link| link.source());
        if let Some(hint) = meerkat_runtime::host_stack::thread_limit_hint(chain) {
            write!(f, "\n{}: {hint}", self.binary)?;
        }
        Ok(())
    }
}

/// Print the `Display` chain of `error` to stderr and return the failure exit
/// status (1), the same status a `Result`-returning `main` produced.
pub fn report_fatal_error(binary: &str, error: &(dyn Error + 'static)) -> ExitCode {
    eprintln!("{}", FatalErrorReport::new(binary, error));
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    enum Outer {
        #[error("session-store has objects outside the ledger; run `rkat storage migrate --apply`")]
        Unledgered {
            #[source]
            inner: Inner,
        },
    }

    #[derive(Debug, thiserror::Error)]
    #[error("table `sessions` is not ledgered")]
    struct Inner {
        #[source]
        io: std::io::Error,
    }

    #[test]
    fn renders_display_chain_with_binary_prefix() {
        let err = Outer::Unledgered {
            inner: Inner {
                io: std::io::Error::other("disk says no"),
            },
        };

        let rendered = FatalErrorReport::new("rkat-rpc", &err).to_string();

        assert_eq!(
            rendered,
            "rkat-rpc: session-store has objects outside the ledger; run `rkat storage migrate --apply`\n\
             rkat-rpc: caused by: table `sessions` is not ledgered\n\
             rkat-rpc: caused by: disk says no"
        );
        assert!(
            !rendered.contains("Unledgered {"),
            "Debug variant shape must not leak into the operator-facing report: {rendered}"
        );
    }

    #[test]
    fn renders_a_sourceless_error_as_one_line() {
        let err: Box<dyn Error> = "internal error: live host missing for --live-ws".into();

        let rendered = FatalErrorReport::new("rkat-rest", err.as_ref()).to_string();

        assert_eq!(
            rendered,
            "rkat-rest: internal error: live host missing for --live-ws"
        );
    }

    #[test]
    fn a_refused_thread_spawn_in_the_chain_adds_the_thread_limit_hint() {
        #[derive(Debug, thiserror::Error)]
        #[error("I/O error: {0}")]
        struct Io(#[source] std::io::Error);

        let err = Io(std::io::Error::from(std::io::ErrorKind::WouldBlock));
        let rendered = FatalErrorReport::new("rkat-rpc", &err).to_string();
        let lines: Vec<&str> = rendered.lines().collect();

        assert_eq!(lines.len(), 3, "{rendered}");
        assert_eq!(lines[0], format!("rkat-rpc: {err}"));
        assert!(lines[1].starts_with("rkat-rpc: caused by: "), "{rendered}");
        assert!(
            lines[2].starts_with("rkat-rpc: hint: the OS refused a new thread or process"),
            "{rendered}"
        );
        assert!(lines[2].contains("worker thread"), "{rendered}");

        let other = Io(std::io::Error::other("disk says no"));
        let rendered = FatalErrorReport::new("rkat-rpc", &other).to_string();
        assert!(!rendered.contains("hint:"), "{rendered}");
    }

    #[test]
    fn report_returns_failure_status() {
        let err = std::io::Error::other("boom");
        assert_eq!(report_fatal_error("rkat-mcp", &err), ExitCode::FAILURE);
    }
}
