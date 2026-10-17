//! A credential command runs as the host, and its token goes where its auth
//! profile and the bindings that use it send it. An ordinary config write
//! can neither add a command nor change what it runs, nor change where its
//! token goes: the profile's provider and auth method, a binding that uses
//! the profile (added, re-pointed at it, or moved to another backend), the
//! backend such a binding names, or the self-hosted server entry that
//! backend names. Each is refused as confinement administration naming the
//! profile, and nothing is written. Removing a binding or the command, and
//! changing what other profiles and bindings use, stays ordinary config.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use meerkat_core::config::{ConfigDelta, ConfigError};
use meerkat_core::{AdministeredSection, Config, ConfigStore, FileConfigStore, MemoryConfigStore};

const STATED: &str = r#"[realm.team.auth.helper]
provider = "openai"
auth_method = "api_key"
source = { kind = "command", program = "/usr/bin/helper", args = ["--token"] }

[realm.team.auth.plain]
provider = "openai"
auth_method = "api_key"
source = { kind = "env", env = "OPENAI_API_KEY" }

[realm.team.backend.openai]
provider = "openai"
backend_kind = "openai_api"
base_url = "https://api.openai.example/v1"

[realm.team.backend.elsewhere]
provider = "openai"
backend_kind = "openai_api"
base_url = "https://elsewhere.example/v1"

[realm.team.binding.main]
backend_profile = "openai"
auth_profile = "helper"

[realm.team.binding.free]
backend_profile = "elsewhere"
auth_profile = "plain"

[self_hosted.servers.box]
base_url = "http://127.0.0.1:9/v1"

[self_hosted.servers.open]
base_url = "http://127.0.0.1:10/v1"

[realm.team.backend.boxed]
provider = "self_hosted"
backend_kind = "self_hosted"
server = "box"

[realm.team.backend.opened]
provider = "self_hosted"
backend_kind = "self_hosted"
server = "open"

[realm.team.auth.box_helper]
provider = "self_hosted"
auth_method = "api_key"
source = { kind = "command", program = "/usr/bin/box-helper" }

[realm.team.auth.anonymous]
provider = "self_hosted"
auth_method = "none"
source = { kind = "platform_default" }

[realm.team.binding.boxed]
backend_profile = "boxed"
auth_profile = "box_helper"

[realm.team.binding.public]
backend_profile = "opened"
auth_profile = "anonymous"
"#;

/// The writes that would send the command's token somewhere else, by the
/// field they change.
fn route_changes() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "the command profile's provider",
            serde_json::json!({"realm": {"team": {"auth": {"helper": {"provider": "anthropic"}}}}}),
        ),
        (
            "the command profile's auth_method",
            serde_json::json!({"realm": {"team": {"auth": {"helper": {"auth_method": "chatgpt_oauth"}}}}}),
        ),
        (
            "the backend of a binding that uses the command",
            serde_json::json!({"realm": {"team": {"binding": {"main": {"backend_profile": "elsewhere"}}}}}),
        ),
        (
            "a new binding that uses the command",
            serde_json::json!({"realm": {"team": {"binding": {"exfil": {
                "backend_profile": "elsewhere",
                "auth_profile": "helper"
            }}}}}),
        ),
        (
            "a binding re-pointed at the command",
            serde_json::json!({"realm": {"team": {"binding": {"free": {"auth_profile": "helper"}}}}}),
        ),
        (
            "the base_url of the backend a command binding names",
            serde_json::json!({"realm": {"team": {"backend": {"openai": {"base_url": "https://attacker.example/v1"}}}}}),
        ),
        (
            "the options of the backend a command binding names",
            serde_json::json!({"realm": {"team": {"backend": {"openai": {"options": {"organization": "attacker"}}}}}}),
        ),
        (
            "the server of the backend a command binding names",
            serde_json::json!({"realm": {"team": {"backend": {"boxed": {"server": "open"}}}}}),
        ),
        (
            "the self-hosted server entry a command binding's backend names",
            serde_json::json!({"self_hosted": {"servers": {"box": {"base_url": "https://attacker.example/v1"}}}}),
        ),
    ]
}

/// What a write's outcome was, without the whole config it admitted.
fn outcome(result: &Result<Config, ConfigError>) -> String {
    match result {
        Ok(_) => "admitted".to_string(),
        Err(error) => format!("refused: {error}"),
    }
}

fn is_command_administration(result: &Result<Config, ConfigError>) -> bool {
    matches!(
        result,
        Err(refused @ ConfigError::CredentialCommandAdministration { .. })
            if refused.administered_section() == Some(AdministeredSection::Confinement)
                && refused.to_string().contains("helper")
    )
}

#[tokio::test]
async fn a_config_write_cannot_change_where_a_credential_command_token_goes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    let store = FileConfigStore::new(path.clone(), meerkat_models::canonical());

    let mut admitted = Vec::new();
    for (field, patch) in route_changes() {
        tokio::fs::write(&path, STATED).await.unwrap();
        let result = store.patch(ConfigDelta(patch)).await;
        if !is_command_administration(&result) {
            admitted.push(format!("{field}: {}", outcome(&result)));
        }
        if tokio::fs::read_to_string(&path).await.unwrap() != STATED {
            admitted.push(format!("{field}: the document was written"));
        }
    }
    assert!(
        admitted.is_empty(),
        "a write that sends the command's token elsewhere is refused and writes nothing:\n{}",
        admitted.join("\n")
    );

    for (change, patch) in [
        (
            "another binding moves to the command's backend",
            serde_json::json!({"realm": {"team": {"binding": {"free": {"backend_profile": "openai"}}}}}),
        ),
        (
            "a backend no command binding names changes",
            serde_json::json!({"realm": {"team": {"backend": {"elsewhere": {"base_url": "https://moved.example/v1"}}}}}),
        ),
        (
            "a server entry no command binding's backend names changes",
            serde_json::json!({"self_hosted": {"servers": {"open": {"base_url": "https://moved.example/v1"}}}}),
        ),
        (
            "the command's binding is removed",
            serde_json::json!({"realm": {"team": {"binding": {"main": null}}}}),
        ),
        (
            "the command and its binding are removed",
            serde_json::json!({"realm": {"team": {"binding": {"main": null}, "auth": {"helper": null}}}}),
        ),
    ] {
        tokio::fs::write(&path, STATED).await.unwrap();
        store
            .patch(ConfigDelta(patch))
            .await
            .unwrap_or_else(|error| panic!("{change} is ordinary config: {error}"));
    }
}

#[tokio::test]
async fn the_memory_store_keeps_where_a_credential_command_token_goes() {
    let stated = Config::from_persisted_toml(STATED).unwrap().0;
    let mut admitted = Vec::new();
    for (field, patch) in route_changes() {
        let store = MemoryConfigStore::new(stated.clone(), meerkat_models::canonical());
        let result = store.patch(ConfigDelta(patch)).await;
        if !is_command_administration(&result) {
            admitted.push(format!("{field}: {}", outcome(&result)));
        }
    }
    assert!(
        admitted.is_empty(),
        "a write that sends the command's token elsewhere is refused:\n{}",
        admitted.join("\n")
    );
}
