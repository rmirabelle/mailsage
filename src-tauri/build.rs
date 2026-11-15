use std::collections::HashMap;

/** A gitignored file of NAME=value lines. An environment variable of the same name wins. */
const SECRETS_FILE: &str = "../secrets/oauth.env";

/** The Google sign-in client, from the app registration in Google Cloud (docs/GOOGLE-SETUP.md). */
const GOOGLE_NAMES: [&str; 2] = ["GOOGLE_CLIENT_ID", "GOOGLE_CLIENT_SECRET"];

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

fn main() {
    println!("cargo:rerun-if-changed={SECRETS_FILE}");
    println!("cargo::rustc-check-cfg=cfg(google_oauth_embedded)");
    let file = secrets_from_file();
    let mut google_complete = true;
    for name in GOOGLE_NAMES {
        println!("cargo:rerun-if-env-changed={name}");
        let value = std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| file.get(name).cloned());
        match value {
            Some(value) => println!("cargo:rustc-env={name}={value}"),
            None => google_complete = false,
        }
    }
    if google_complete {
        println!("cargo:rustc-cfg=google_oauth_embedded");
    }
    tauri_build::build()
}
