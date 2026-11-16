use std::collections::HashMap;

/** A gitignored file of NAME=value lines. An environment variable of the same name wins. */
const SECRETS_FILE: &str = "../secrets/oauth.env";

/** The Google sign-in client, from the app registration in Google Cloud (docs/GOOGLE-SETUP.md). */
const GOOGLE_NAMES: [&str; 2] = ["GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET"];
/** The Microsoft sign-in client, from the app registration in Microsoft Entra (docs/MICROSOFT-SETUP.md). It has no client secret. */
const MICROSOFT_NAMES: [&str; 1] = ["MICROSOFT_CLIENT_ID"];
/** The second Microsoft registration: the one that permits work or school accounts (Microsoft 365). */
const MICROSOFT_WORK_NAMES: [&str; 1] = ["MICROSOFT_WORK_CLIENT_ID"];

fn secrets_from_file() -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(SECRETS_FILE) else {
        return HashMap::new();
    };
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

/**
 * Gives the values of one sign-in client to the code, and sets the mark
 * `cfg_name` when all of them are there. The code embeds them with `obfstr`.
 */
fn embed(names: &[&str], cfg_name: &str, file: &HashMap<String, String>) {
    println!("cargo::rustc-check-cfg=cfg({cfg_name})");
    let mut complete = true;
    for name in names {
        println!("cargo:rerun-if-env-changed={name}");
        let value = std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| file.get(*name).cloned());
        match value {
            Some(value) => println!("cargo:rustc-env={name}={value}"),
            None => complete = false,
        }
    }
    if complete {
        println!("cargo:rustc-cfg={cfg_name}");
    }
}

fn main() {
    println!("cargo:rerun-if-changed={SECRETS_FILE}");
    let file = secrets_from_file();
    embed(&GOOGLE_NAMES, "google_oauth_embedded", &file);
    embed(&MICROSOFT_NAMES, "microsoft_oauth_embedded", &file);
    embed(&MICROSOFT_WORK_NAMES, "microsoft_work_oauth_embedded", &file);
    tauri_build::build()
}
