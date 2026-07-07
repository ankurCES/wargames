//! `~/.blumi/settings.json` access.
//!
//! The path is **hardcoded** — there is no environment-variable override.
//! Every blumi app on this device uses the same file, and any divergence
//! would break the shared-config invariant.
//!
//! The loader is *typed*: it does not return a generic `serde_json::Value`.
//! The contract every blumi app should conform to is a strict subset of
//! the real settings file (LLM providers, router, optional voice/TTS).
//!
//! Missing file → exit code 2 (not 1). 1 is reserved for "the model
//! said no"; 2 means "the environment is not set up".
//!
//! Spec-driven fields (light/heavy/judge router entries, TTS provider,
//! Voice struct) are part of the settings.json schema even when no
//! caller reads them yet — keep them deserialisable so a settings.json
//! authored against the full schema still loads.
#![allow(dead_code)]

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The single, hardcoded path. Do not change this.
pub fn blumi_settings_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home"));
    home.join(".blumi").join("settings.json")
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlumiSettings {
    #[serde(default)]
    pub providers: Providers,
    #[serde(default)]
    pub router: Router,
    #[serde(default)]
    pub voice: Option<Voice>,
}

impl BlumiSettings {
    pub fn from_default_path() -> Result<Self, ConfigError> {
        Self::from_path(&blumi_settings_path())
    }

    pub fn from_path(p: &Path) -> Result<Self, ConfigError> {
        let raw = std::fs::read_to_string(p)?;
        let s: BlumiSettings = serde_json::from_str(&raw)?;
        Ok(s)
    }

    /// Pick the light router's provider + model as the default for the
    /// Soviet commander (matches the JS impl's `router.light`).
    pub fn light_model(&self) -> Option<(&'static str, String)> {
        let provider = self.router.light.provider.as_deref()?;
        let model = self.router.light.model.clone()?;
        let provider = provider.to_string();
        // Return a `'static str` for the provider by leaking into a Box<str>
        // is overkill — callers copy the name into reqwest anyway. We just
        // return owned strings.
        let _ = provider;
        Some(("router.light", model))
    }

    /// Resolve an LLM provider entry by name (e.g. "minimax", "azure-foundry").
    pub fn provider(&self, name: &str) -> Option<&Provider> {
        self.providers.0.get(name)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Providers(pub std::collections::HashMap<String, Provider>);

#[derive(Debug, Clone, Deserialize)]
pub struct Provider {
    pub api_key: String,
    pub base_url: String,
    #[serde(default = "default_kind")]
    pub kind: String, // "anthropic" | "openai" | ...
    #[serde(default)]
    pub models: Vec<String>,
}

fn default_kind() -> String {
    "anthropic".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Router {
    #[serde(default)]
    pub light: RouterEntry,
    #[serde(default)]
    pub heavy: RouterEntry,
    #[serde(default)]
    pub judge: RouterEntry,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RouterEntry {
    pub model: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Voice {
    #[serde(default)]
    pub enabled: bool,
    pub tts_api_key: Option<String>,
    pub tts_voice: Option<String>,
    #[serde(default)]
    pub tts_provider: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("~/.blumi/settings.json not found at {0} — this device's blumi apps all share that single config")]
    Missing(PathBuf),
    #[error("io error reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            ConfigError::Missing(blumi_settings_path())
        } else {
            ConfigError::Io(blumi_settings_path(), e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpfile(name: &str, contents: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("wargames-blumi-{}-{}.json", std::process::id(), name));
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        p
    }

    #[test]
    fn loads_minimax_provider() {
        let raw = r#"{
            "providers": {
                "minimax": {
                    "api_key": "sk-test",
                    "base_url": "https://api.minimax.io/anthropic",
                    "kind": "anthropic"
                }
            },
            "router": { "light": { "model": "MiniMax-M3", "provider": "minimax" } }
        }"#;
        let p = tmpfile("minimax", raw);
        let s = BlumiSettings::from_path(&p).unwrap();
        let prov = s.provider("minimax").unwrap();
        assert_eq!(prov.api_key, "sk-test");
        assert_eq!(prov.base_url, "https://api.minimax.io/anthropic");
        assert_eq!(s.router.light.model.as_deref(), Some("MiniMax-M3"));
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn missing_file_is_a_typed_error() {
        let p = PathBuf::from("/tmp/wargames-blumi-does-not-exist-xyz.json");
        let err = BlumiSettings::from_path(&p).unwrap_err();
        match err {
            ConfigError::Missing(_) | ConfigError::Io(_, _) => {}
            other => panic!("expected Missing/Io, got {:?}", other),
        }
    }

    #[test]
    fn hardcoded_path_targets_blumi_settings() {
        let p = blumi_settings_path();
        assert!(p.ends_with(".blumi/settings.json"));
    }

    /// The committed `examples/settings.sample.json` must round-trip through
    /// `BlumiSettings::from_path` and yield non-empty providers + a router
    /// whose `light` entry resolves. This pins the docs sample to the same
    /// schema the binary consumes — if `examples/settings.sample.json`
    /// drifts out of sync with `Provider`/`Router`/`RouterEntry`, this test
    /// fails before any user copy-pastes a broken file into `~/.blumi/`.
    #[test]
    fn documented_sample_parses_and_resolves_router() {
        // Walk up from `crates/wargames-tui/` to the workspace root, then
        // into `examples/`. Cargo sets CARGO_MANIFEST_DIR at compile time.
        let manifest = std::env::var("CARGO_MANIFEST_DIR")
            .expect("CARGO_MANIFEST_DIR is set by cargo");
        let sample = std::path::Path::new(&manifest)
            .join("../../examples/settings.sample.json");
        let s = BlumiSettings::from_path(&sample)
            .unwrap_or_else(|e| panic!("sample at {} did not parse: {e}", sample.display()));
        // At least one provider must be present and every one of them must
        // have a non-empty api_key + base_url (the binary will reject empty
        // keys silently on the wire).
        assert!(!s.providers.0.is_empty(), "sample must list at least one provider");
        for (name, p) in &s.providers.0 {
            assert!(!p.api_key.is_empty(), "provider {name} has empty api_key");
            assert!(!p.base_url.is_empty(), "provider {name} has empty base_url");
        }
        // router.light.{model,provider} must both resolve, and the named
        // provider must exist — `from_settings` rejects unknown names.
        let light_provider = s
            .router
            .light
            .provider
            .as_deref()
            .expect("sample must set router.light.provider");
        let light_model = s
            .router
            .light
            .model
            .clone()
            .expect("sample must set router.light.model");
        assert!(
            s.provider(light_provider).is_some(),
            "router.light.provider {light_provider:?} must reference a key under providers"
        );
        // `model` should also appear in the provider's `models` list — this
        // is a documentation invariant, not strictly required by the loader,
        // but if a user copy-pastes a `model` that the provider doesn't
        // advertise they'll get opaque 4xx from the upstream API.
        let prov = s.provider(light_provider).unwrap();
        assert!(
            prov.models.iter().any(|m| m == &light_model),
            "router.light.model {light_model:?} must appear in providers.{light_provider}.models"
        );
    }
}