//! How a Git skill invocation gives git the HTTPS token for the configured
//! repository's host.
//!
//! On Unix the token reaches git through git's own credential protocol: the
//! invocation's options empty git's credential helper list and name one
//! helper for the repository's host, git's `credential-cache` client, which
//! asks a socket the host serves for that one invocation
//! ([`CredentialSocket`]). The token is then on no command line, in no
//! environment variable, in no file and behind no descriptor a process git
//! starts inherits, and no helper the host's git configuration names is asked
//! for, or sent, a credential. Elsewhere (no Unix-domain sockets, and no
//! `credential-cache` in Git for Windows) the token is an `Authorization`
//! header option on the invocation's own command line, scoped to the
//! configured repository URL and with redirects not followed: visible to
//! processes of the same user for that invocation only, and persisted
//! nowhere.

use std::path::Path;

use meerkat_core::skills::SkillError;

/// The user name git sends with the token, as GitHub expects for an
/// installation or a personal access token.
const USERNAME: &str = "x-access-token";

fn load_error(message: impl Into<String>) -> SkillError {
    SkillError::Load(message.into().into())
}

/// The configured repository's HTTPS token, the `host[:port]` git asks for
/// it with, and the repository URL it is for.
pub(super) struct HttpsToken<'a> {
    origin: &'a str,
    /// The configured URL without user information, query or fragment.
    remote: String,
    token: &'a str,
}

impl<'a> HttpsToken<'a> {
    /// The token for `repo_url` when it is an `https://` URL; `None` for any
    /// other transport, which is never sent the token. A URL whose host is
    /// not a plain `host[:port]`, or a token that is empty or holds a control
    /// character, is refused: git could not take either as one value.
    pub(super) fn for_repository(
        repo_url: &'a str,
        token: &'a str,
    ) -> Result<Option<Self>, SkillError> {
        let Some(rest) = repo_url.strip_prefix("https://") else {
            return Ok(None);
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let origin = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let path = rest[authority.len()..]
            .split(['?', '#'])
            .next()
            .unwrap_or_default();
        let plain =
            |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']');
        if origin.is_empty() || !origin.chars().all(plain) {
            return Err(load_error(
                "git skill https token refused: the repository URL names no plain host",
            ));
        }
        if token.is_empty() || token.chars().any(char::is_control) {
            return Err(load_error(
                "git skill https token refused: it is empty or holds a control character",
            ));
        }
        Ok(Some(Self {
            origin,
            remote: format!("https://{origin}{path}"),
            token,
        }))
    }

    /// The `-c` options an invocation takes before its subcommand, which git
    /// never writes to a repository's config: an emptied credential helper
    /// list, so no helper the host's git configuration names is asked for or
    /// sent a credential; no askpass program; and the token by `route`: the
    /// socket for the repository's host alone, or a header for the
    /// configured repository URL alone (another remote or a submodule of the
    /// same invocation never gets it) with no redirect followed, so it is
    /// never replayed to another host.
    pub(super) fn config_options(&self, route: &TokenRoute<'_>) -> Result<Vec<String>, SkillError> {
        let origin = self.origin;
        let mut options = vec!["credential.helper=".to_owned(), "core.askPass=".to_owned()];
        match route {
            TokenRoute::Socket(socket) => {
                // git runs a helper through `sh -c`, so the path is
                // single-quoted and may hold no quote or control character.
                let socket = socket
                    .to_str()
                    .filter(|path| !path.contains('\'') && !path.chars().any(char::is_control))
                    .ok_or_else(|| {
                        load_error(format!(
                            "git credential socket refused: {} is not a path git's credential helper can be given",
                            socket.display()
                        ))
                    })?;
                options.push(format!(
                    "credential.https://{origin}.helper=cache --socket='{socket}'"
                ));
            }
            TokenRoute::CommandLine => {
                // git takes `-c <key>=<value>` at the first `=`, and the URL
                // is part of the key.
                if self.remote.contains('=') {
                    return Err(load_error(
                        "git skill https token refused: the repository URL holds '=', which git cannot scope an option to",
                    ));
                }
                options.push("http.followRedirects=false".to_owned());
                options.push(format!(
                    "http.{}.extraHeader=Authorization: Basic {}",
                    self.remote,
                    base64(format!("{USERNAME}:{}", self.token).as_bytes())
                ));
            }
        }
        Ok(options)
    }
}

/// How an invocation receives the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenRoute<'a> {
    /// git's `credential-cache` client asks the socket at this path, which
    /// the host serves for the one invocation.
    Socket(&'a Path),
    /// The token is an `Authorization` header option on the invocation's
    /// own command line, scoped to the configured repository URL with no
    /// redirect followed, and so, as git passes its `-c` options on, in the
    /// environment of the git processes it starts: visible to processes of
    /// the same user for that invocation only. git persists it nowhere.
    CommandLine,
}

impl<'a> TokenRoute<'a> {
    /// The served socket where the platform has one, else the command line.
    pub(super) fn new(socket: Option<&'a Path>) -> Self {
        socket.map_or(Self::CommandLine, Self::Socket)
    }
}

/// Standard base64 with padding (RFC 4648), for a basic `Authorization`
/// header.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |index: usize| u32::from(chunk.get(index).copied().unwrap_or(0));
        let group = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for position in 0..4 {
            if position <= chunk.len() {
                let sextet = (group >> (18 - 6 * position)) & 63;
                encoded.push(char::from(ALPHABET[sextet as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[cfg(unix)]
pub(super) use served::CredentialSocket;

/// Where a credential socket's directory may go, in order: the system
/// temporary directory, whose short path fits a Unix socket address (the
/// per-user one on macOS), then the skill cache. Neither is storage: the
/// directory lives for one git invocation. [`CredentialSocket::serve`]
/// skips one a confined process could change.
#[cfg(unix)]
pub(super) fn socket_parents(cache_dir: &Path) -> [std::path::PathBuf; 2] {
    [std::env::temp_dir(), cache_dir.to_path_buf()]
}

#[cfg(unix)]
mod served {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use meerkat_core::confinement::UntrustedPathFn;
    use meerkat_core::skills::SkillError;

    use super::{HttpsToken, USERNAME, load_error};

    /// How long the server sleeps between polls for a connection.
    const POLL: Duration = Duration::from_millis(10);
    /// How long the server waits on one connection's request or reply.
    const IO_TIMEOUT: Duration = Duration::from_secs(5);
    /// The most one request may hold; git's are a few lines.
    const MAX_REQUEST: u64 = 64 * 1024;

    /// A socket the host serves git's `credential-cache` client on for one
    /// invocation: a `get` for the repository's `https://` host is answered
    /// with the token, and nothing else is. It lives in a new directory only
    /// the host's user may enter (created `0700` in one step, under a name no
    /// one can predict), and dropping the value stops the server and removes
    /// the socket and the directory, however the invocation ended.
    pub(in crate::source::git) struct CredentialSocket {
        directory: PathBuf,
        socket: PathBuf,
        stop: Arc<AtomicBool>,
        server: Option<JoinHandle<()>>,
    }

    impl CredentialSocket {
        /// Serve `token` from a new directory under the first of `parents`
        /// that holds the socket and that no confined process could change
        /// (`untrusted`): a process that could replace the socket could make
        /// git's client start a cache daemon of its own there.
        pub(in crate::source::git) fn serve(
            token: &HttpsToken<'_>,
            parents: &[PathBuf],
            untrusted: Option<&UntrustedPathFn>,
        ) -> Result<Self, SkillError> {
            let mut refusal = load_error(
                "git credential socket refused: every directory for it is one a confined process could change",
            );
            for parent in parents {
                if untrusted.is_some_and(|untrusted| untrusted(parent, true)) {
                    continue;
                }
                match Self::serve_in(parent, token) {
                    Ok(served) => return Ok(served),
                    Err(error) => {
                        refusal = load_error(format!(
                            "git credential socket under {}: {error}",
                            parent.display()
                        ));
                    }
                }
            }
            Err(refusal)
        }

        fn serve_in(parent: &Path, token: &HttpsToken<'_>) -> std::io::Result<Self> {
            let directory = private_directory(parent)?;
            let mut served = Self {
                socket: directory.join("socket"),
                directory,
                stop: Arc::new(AtomicBool::new(false)),
                server: None,
            };
            let listener = UnixListener::bind(&served.socket)?;
            listener.set_nonblocking(true)?;
            let stop = Arc::clone(&served.stop);
            let origin = token.origin.to_owned();
            let secret = token.token.to_owned();
            served.server = Some(
                std::thread::Builder::new()
                    .name("git-credential".to_owned())
                    .spawn(move || answer_requests(&listener, &stop, &origin, &secret))?,
            );
            Ok(served)
        }

        /// The socket's path, for git's `credential-cache --socket`.
        pub(in crate::source::git) fn path(&self) -> &Path {
            &self.socket
        }
    }

    impl Drop for CredentialSocket {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(server) = self.server.take() {
                let _ = server.join();
            }
            let _ = std::fs::remove_file(&self.socket);
            let _ = std::fs::remove_dir(&self.directory);
        }
    }

    /// A new directory under `parent` only the host's user may enter,
    /// created with that mode in one step and under a name no one can
    /// predict, so nothing can be waiting at its path.
    fn private_directory(parent: &Path) -> std::io::Result<PathBuf> {
        use std::hash::{BuildHasher, RandomState};
        use std::os::unix::fs::DirBuilderExt;

        let mut attempts = 0;
        loop {
            // A fresh `RandomState` holds keys this process drew at random,
            // so the name is a keyed hash no other process can compute.
            let name = format!(
                "git-credential-{:016x}",
                RandomState::new().hash_one(std::process::id())
            );
            let directory = parent.join(name);
            match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
                Ok(()) => return Ok(directory),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempts < 8 => {
                    attempts += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn answer_requests(listener: &UnixListener, stop: &AtomicBool, origin: &str, token: &str) {
        while !stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _)) => answer(&mut stream, origin, token),
                Err(_) => std::thread::sleep(POLL),
            }
        }
    }

    fn answer(stream: &mut UnixStream, origin: &str, token: &str) {
        // An accepted connection may keep the listener's non-blocking mode.
        if stream.set_nonblocking(false).is_err()
            || stream.set_read_timeout(Some(IO_TIMEOUT)).is_err()
            || stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
        {
            return;
        }
        let mut request = Vec::new();
        if Read::by_ref(stream)
            .take(MAX_REQUEST)
            .read_to_end(&mut request)
            .is_err()
        {
            return;
        }
        let reply = reply(&String::from_utf8_lossy(&request), origin, token);
        let _ = stream.write_all(reply.as_bytes());
    }

    /// The reply to one request git's `credential-cache` client relays (an
    /// `action=` line, a `timeout=` line and git's credential request). A
    /// `get` for the repository's `https://` host is answered with the
    /// token; any other `get` with `quit`, so git asks no other helper and
    /// prompts for nothing; a `store` or an `erase` with nothing, so no
    /// credential is kept or dropped anywhere.
    pub(super) fn reply(request: &str, origin: &str, token: &str) -> String {
        let field = |name: &str| {
            request
                .lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
        };
        match field("action") {
            Some("get")
                if field("protocol") == Some("https")
                    && field("host").is_some_and(|host| host.eq_ignore_ascii_case(origin)) =>
            {
                format!("username={USERNAME}\npassword={token}\n")
            }
            Some("get") => "quit=1\n".to_owned(),
            _ => String::new(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    const TOKEN: &str = "ghs-unit-token";

    fn token(url: &str) -> HttpsToken<'_> {
        match HttpsToken::for_repository(url, TOKEN) {
            Ok(Some(token)) => token,
            other => panic!(
                "{url} takes the token: {:?}",
                other.map(|token| token.is_some())
            ),
        }
    }

    /// Only an `https://` repository is given the token, for its plain host
    /// and port without any user information; a host or a token git could
    /// not take as one value is refused.
    #[test]
    fn the_token_is_for_the_https_host_alone() {
        for (url, origin) in [
            ("https://github.com/org/skills.git", "github.com"),
            (
                "https://git.example.com:8443/skills",
                "git.example.com:8443",
            ),
            ("https://someone@example.com/skills?ref=main", "example.com"),
            ("https://[::1]:8443/skills", "[::1]:8443"),
        ] {
            assert_eq!(token(url).origin, origin, "{url}");
        }
        for url in [
            "http://example.com/skills.git",
            "ssh://git@example.com/skills.git",
            "git@example.com:org/skills.git",
            "file:///srv/skills.git",
        ] {
            assert!(
                matches!(HttpsToken::for_repository(url, TOKEN), Ok(None)),
                "{url} is never sent the token"
            );
        }
        for url in [
            "https:///skills.git",
            "https://exa'mple.com/skills.git",
            "https://a=b/skills",
        ] {
            assert!(HttpsToken::for_repository(url, TOKEN).is_err(), "{url}");
        }
        for bad in ["", "line\nbreak", "tab\ttoken"] {
            assert!(
                HttpsToken::for_repository("https://example.com/s.git", bad).is_err(),
                "{bad:?}"
            );
        }
    }

    /// Both routes empty git's credential helper list and turn askpass off
    /// before they name the one way the token reaches git: the Unix form
    /// names git's `credential-cache` client and the served socket, scoped to
    /// the repository's host, and puts no token in an option; the form for
    /// platforms without Unix-domain sockets puts it in a host-scoped
    /// `Authorization` header option, which git persists nowhere.
    #[test]
    fn each_route_names_the_token_for_the_repository_host_only() {
        let https = token("https://git.example.com:8443/org/skills.git");
        let socket = Path::new("/run/user/1000/git-credential-0123456789abcdef/socket");
        let options = https
            .config_options(&TokenRoute::new(Some(socket)))
            .unwrap_or_default();
        assert_eq!(
            options,
            [
                "credential.helper=",
                "core.askPass=",
                "credential.https://git.example.com:8443.helper=cache --socket='/run/user/1000/git-credential-0123456789abcdef/socket'",
            ]
        );
        assert!(options.iter().all(|option| !option.contains(TOKEN)));

        let options = https
            .config_options(&TokenRoute::new(None))
            .unwrap_or_default();
        assert_eq!(
            options,
            [
                "credential.helper=",
                "core.askPass=",
                "http.followRedirects=false",
                // base64("x-access-token:ghs-unit-token"), for the configured
                // repository URL alone.
                "http.https://git.example.com:8443/org/skills.git.extraHeader=Authorization: Basic eC1hY2Nlc3MtdG9rZW46Z2hzLXVuaXQtdG9rZW4=",
            ]
        );
        assert_eq!(
            token("https://someone@example.com/skills?ref=main#top")
                .config_options(&TokenRoute::CommandLine)
                .unwrap_or_default()
                .last()
                .map(String::as_str),
            Some(
                "http.https://example.com/skills.extraHeader=Authorization: Basic eC1hY2Nlc3MtdG9rZW46Z2hzLXVuaXQtdG9rZW4="
            ),
            "the header's scope keeps no user, query or fragment"
        );
        assert!(
            token("https://example.com/a=b/skills.git")
                .config_options(&TokenRoute::CommandLine)
                .is_err(),
            "a URL git could not take as part of an option key is refused"
        );

        for refused in ["/tmp/it's/socket", "/tmp/new\nline/socket"] {
            assert!(
                https
                    .config_options(&TokenRoute::Socket(Path::new(refused)))
                    .is_err(),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn base64_pads_every_length() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    /// The served socket answers a `get` for the repository's `https://` host
    /// with the token and nothing else: another host or plain `http` gets
    /// `quit`, and a `store` or `erase` (git approving or rejecting what it
    /// used) gets nothing.
    #[cfg(unix)]
    #[test]
    fn the_socket_answers_only_a_get_for_the_repository_host() {
        let reply = |request: &str| served::reply(request, "git.example.com:8443", TOKEN);
        assert_eq!(
            reply(
                "action=get\ntimeout=900\nprotocol=https\nhost=Git.Example.com:8443\npath=org/skills.git\n"
            ),
            format!("username=x-access-token\npassword={TOKEN}\n")
        );
        for request in [
            "action=get\ntimeout=900\nprotocol=https\nhost=elsewhere.example\n",
            "action=get\ntimeout=900\nprotocol=http\nhost=git.example.com:8443\n",
            "action=get\ntimeout=900\nprotocol=https\nhost=git.example.com\n",
            "action=get\ntimeout=900\nprotocol=https\nhostname=git.example.com:8443\n",
            "action=get\ntimeout=900\n",
        ] {
            assert_eq!(reply(request), "quit=1\n", "{request:?}");
        }
        for request in [
            "action=store\ntimeout=900\nprotocol=https\nhost=git.example.com:8443\nusername=x\npassword=y\n",
            "action=erase\ntimeout=900\nprotocol=https\nhost=git.example.com:8443\n",
            "action=exit\n",
            "",
        ] {
            assert_eq!(reply(request), "", "{request:?}");
        }
    }
}
