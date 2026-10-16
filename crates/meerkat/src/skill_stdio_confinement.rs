//! A stdio skill source's process under the host's `mcp_stdio`
//! confinement, as a local MCP stdio server runs under it.

use std::path::PathBuf;
use std::time::Duration;

use meerkat_core::confinement::{ConfinementRefusal, ExecutionConfinement};
use meerkat_core::skills::SkillError;
use meerkat_sandbox::CompiledConfinement;
use meerkat_skills::{StdioSkillLaunch, StdioSkillOutput, StdioSkillRunner};

/// Runs a stdio skill source's process under a required confinement: the
/// compiled requirement, in the kind's working directory (the source's own
/// `cwd` does not apply), with only the source's configured environment and
/// its program named by absolute path. Its diagnostics are discarded, as a
/// confined MCP stdio server's are. A launch the platform cannot confine as
/// required is refused and never runs as the host.
pub(crate) struct ConfinedStdioSkillRunner {
    compiled: Result<CompiledConfinement, ConfinementRefusal>,
    working_directory: PathBuf,
}

impl ConfinedStdioSkillRunner {
    pub(crate) fn new(requirement: &ExecutionConfinement, working_directory: PathBuf) -> Self {
        let compiled = CompiledConfinement::compile(requirement).and_then(|compiled| {
            if !working_directory.is_absolute()
                || working_directory
                    .as_os_str()
                    .as_encoded_bytes()
                    .contains(&0)
            {
                return Err(ConfinementRefusal::InvalidLaunch);
            }
            Ok(compiled)
        });
        Self {
            compiled,
            working_directory,
        }
    }
}

fn refused(refusal: ConfinementRefusal) -> SkillError {
    SkillError::Load(format!("stdio skill source launch refused: {refusal}").into())
}

#[async_trait::async_trait]
impl StdioSkillRunner for ConfinedStdioSkillRunner {
    async fn run(
        &self,
        launch: &StdioSkillLaunch,
        input: &[u8],
        timeout: Duration,
    ) -> Result<StdioSkillOutput, SkillError> {
        let compiled = self
            .compiled
            .as_ref()
            .map_err(|refusal| refused(*refusal))?;
        let spec = meerkat_sandbox::ProcessLaunchSpec::new(
            PathBuf::from(&launch.command),
            launch.args.iter().map(Into::into).collect(),
            self.working_directory.clone(),
            launch
                .env
                .iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        )
        .map_err(refused)?;
        let prepared = compiled.bind_launch(spec).map_err(refused)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let child = prepared
                .spawn_with_io(meerkat_sandbox::SpawnIo {
                    stdin: meerkat_sandbox::StdioMode::Piped,
                    stdout: meerkat_sandbox::StdioMode::Piped,
                    stderr: meerkat_sandbox::StdioMode::Null,
                })
                .map_err(|error| {
                    SkillError::Load(format!("stdio skill source spawn failed: {error}").into())
                })?;
            exchange(child.into(), input, timeout).await
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (prepared, input, timeout);
            Err(refused(ConfinementRefusal::UnsupportedRequirement))
        }
    }
}

/// Hand `input` to the confined process, collect its output until it exits,
/// and kill it once `timeout` passes.
#[cfg(any(target_os = "linux", target_os = "macos"))]
async fn exchange(
    mut child: meerkat_sandbox::ProcessChild,
    input: &[u8],
    timeout: Duration,
) -> Result<StdioSkillOutput, SkillError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let failed = |step: &str, error: std::io::Error| {
        SkillError::Load(format!("stdio skill source {step} failed: {error}").into())
    };
    let Some(mut stdout) = child.take_stdout() else {
        let _ = child.kill().await;
        return Err(SkillError::Load(
            "stdio skill source stdout pipe unavailable".into(),
        ));
    };
    let reader = tokio::spawn(async move {
        let mut output = String::new();
        stdout.read_to_string(&mut output).await.map(|_| output)
    });
    if let Some(mut stdin) = child.take_stdin()
        && let Err(error) = stdin.write_all(input).await
    {
        let _ = child.kill().await;
        reader.abort();
        return Err(failed("write", error));
    }
    let status = tokio::select! {
        status = child.wait() => status.map_err(|error| failed("wait", error))?,
        () = tokio::time::sleep(timeout) => {
            let _ = child.kill().await;
            reader.abort();
            return Err(SkillError::Load("stdio skill source timed out".into()));
        }
    };
    let stdout = reader
        .await
        .map_err(|error| {
            SkillError::Load(format!("stdio stdout task join failed: {error}").into())
        })?
        .map_err(|error| failed("read", error))?;
    Ok(StdioSkillOutput {
        status,
        stdout,
        stderr: String::new(),
    })
}
