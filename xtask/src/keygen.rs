//! `cargo xtask keygen`: a new Ed25519 release signing key pair. The 32-byte secret seed is made
//! from the OS's random source in memory, sent to 1Password through its local MCP server
//! (`mcp.rs`) as `SHELLMAN_RELEASE_SIGNING_KEY` in a new Environment named [`ENVIRONMENT`], and
//! wiped. It is never printed, logged or written to a file. Only the public key is printed, to be
//! pasted into `crates/update/src/sig.rs` (`PUBLIC_KEY`).
//!
//! It refuses when an Environment of that name already exists (replacing the key is a deliberate
//! act: rename the old Environment first, in the 1Password app).

use crate::mcp::{self, Client};
use serde_json::json;
use shellman_update_core::{hex, sig};

pub const ENVIRONMENT: &str = "shellman/release-signing";
pub const SECRET: &str = "SHELLMAN_RELEASE_SIGNING_KEY";

pub fn run(_args: &[String]) -> i32 {
    match keygen() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("keygen: {e}");
            1
        }
    }
}

fn keygen() -> Result<(), String> {
    let mut op = Client::start()?;
    println!("1Password: connected; asking to sign in (approve in the 1Password app if it asks)");
    let auth = op.call("authenticate", json!({}))?;
    let account = mcp::find_str(&auth, &["account_id", "accountId"]).ok_or("1Password did not say which account")?;
    let list = op.call("list_environments", json!({ "accountId": account }))?;
    let mut named = Vec::new();
    mcp::objects_with(&list, "name", &mut named);
    mcp::objects_with(&list, "environment_name", &mut named);
    let clash = named.iter().any(|m| ["name", "environment_name"].iter().any(|k| m.get(*k).and_then(|v| v.as_str()) == Some(ENVIRONMENT)))
        || list.as_str().is_some_and(|s| s.contains(ENVIRONMENT));
    if clash {
        return Err(format!("an Environment named '{ENVIRONMENT}' already exists; nothing was changed. To replace the key, rename that one in 1Password first"));
    }

    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| format!("no random source ({e})"))?;
    let public = sig::public_of(&seed);
    let mut secret_hex = hex(&seed);
    seed.fill(0);

    let created = op.call("create_environment", json!({ "accountId": account, "environmentName": ENVIRONMENT }))?;
    let env = mcp::find_str(&created, &["environment_id", "environmentId", "id"]).ok_or("1Password did not say the new Environment's id")?;
    let stored = op.call("append_secrets", json!({
        "accountId": account,
        "environmentId": env,
        "secrets": [
            { "name": SECRET, "value": secret_hex, "concealed": true },
            { "name": "SHELLMAN_RELEASE_PUBLIC_KEY", "value": hex(&public), "concealed": false }
        ]
    }));
    // Wipe the only copy outside 1Password, whatever happened.
    // SAFETY: overwriting ASCII hex digits with '0' keeps the string valid UTF-8.
    unsafe { secret_hex.as_bytes_mut().fill(b'0') };
    drop(secret_hex);
    stored?;

    let names = op.call("list_secrets", json!({ "accountId": account, "environmentId": env }))?;
    let names_text = names.to_string();
    if !names_text.contains(SECRET) {
        return Err(format!("the key was sent but 1Password does not list {SECRET} in '{ENVIRONMENT}'; do not use this key"));
    }
    println!("stored {SECRET} (concealed) in the 1Password Environment '{ENVIRONMENT}' (id {env})");
    println!("public key (hex): {}", hex(&public));
    println!("fingerprint:      {}", sig::fingerprint(&public));
    println!("for crates/update/src/sig.rs:");
    let bytes: Vec<String> = public.iter().map(|b| format!("0x{b:02x}")).collect();
    println!("pub const PUBLIC_KEY: [u8; 32] = [\n    {},\n    {},\n];", bytes[..16].join(", "), bytes[16..].join(", "));
    Ok(())
}
