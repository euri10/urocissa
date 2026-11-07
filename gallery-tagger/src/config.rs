//! Runtime configuration from environment variables.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::api::Limits;
use crate::openvino_backend::BackendConfig;

pub const DEFAULT_MODEL_DIR: &str = "/opt/gallery-tagger/model";
pub const DEFAULT_LISTEN: &str = "0.0.0.0:5674";
pub const DEFAULT_DEVICES: &str = "GPU,CPU";

#[derive(Debug, Clone)]
pub struct Config {
    pub model_dir: PathBuf,
    pub listen: SocketAddr,
    pub backend: BackendConfig,
    pub limits: Limits,
}

impl Config {
    /// Reads `TAGGER_MODEL_DIR`, `TAGGER_LISTEN`, `TAGGER_DEVICES`
    /// (comma-separated, tried in order), `TAGGER_CPU_THREADS`,
    /// `TAGGER_CPU_PRECISION` (`f32` or `bf16`), `TAGGER_CACHE_DIR` and
    /// `TAGGER_MAX_IMAGE_BYTES`.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get = |key: &str, default: &str| lookup(key).unwrap_or_else(|| default.to_owned());
        let devices: Vec<String> = get("TAGGER_DEVICES", DEFAULT_DEVICES)
            .split(',')
            .map(|device| device.trim().to_uppercase())
            .filter(|device| !device.is_empty())
            .collect();
        if devices.is_empty() {
            bail!("TAGGER_DEVICES must name at least one device");
        }
        let cpu_threads = match lookup("TAGGER_CPU_THREADS") {
            None => None,
            Some(raw) => {
                let threads: usize = raw
                    .parse()
                    .context("TAGGER_CPU_THREADS must be a positive integer")?;
                if threads == 0 {
                    bail!("TAGGER_CPU_THREADS must be a positive integer");
                }
                Some(threads)
            }
        };
        let cpu_precision =
            match lookup("TAGGER_CPU_PRECISION").map(|raw| raw.trim().to_lowercase()) {
                None => None,
                Some(precision) if precision == "f32" || precision == "bf16" => Some(precision),
                Some(other) => bail!("TAGGER_CPU_PRECISION must be f32 or bf16, not {other:?}"),
            };
        let mut limits = Limits::default();
        if let Some(raw) = lookup("TAGGER_MAX_IMAGE_BYTES") {
            limits.max_image_bytes = raw
                .parse()
                .context("TAGGER_MAX_IMAGE_BYTES must be an integer")?;
        }
        Ok(Self {
            model_dir: PathBuf::from(get("TAGGER_MODEL_DIR", DEFAULT_MODEL_DIR)),
            listen: get("TAGGER_LISTEN", DEFAULT_LISTEN)
                .parse()
                .context("TAGGER_LISTEN must be host:port")?,
            backend: BackendConfig {
                devices,
                cpu_threads,
                cpu_precision,
                cache_dir: lookup("TAGGER_CACHE_DIR").filter(|dir| !dir.trim().is_empty()),
            },
            limits,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(pairs: &[(&str, &str)]) -> Result<Config> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Config::from_lookup(|key| map.get(key).cloned())
    }

    #[test]
    fn defaults_prefer_gpu_then_cpu() {
        let config = config(&[]).unwrap();
        assert_eq!(config.backend.devices, ["GPU", "CPU"]);
        assert_eq!(config.backend.cpu_threads, None);
        assert_eq!(config.listen.port(), 5674);
    }

    #[test]
    fn parses_device_list_and_threads() {
        let config = config(&[("TAGGER_DEVICES", " cpu , "), ("TAGGER_CPU_THREADS", "1")]).unwrap();
        assert_eq!(config.backend.devices, ["CPU"]);
        assert_eq!(config.backend.cpu_threads, Some(1));
        let config = super::Config::from_lookup(|key| {
            (key == "TAGGER_CPU_PRECISION").then(|| "F32".to_owned())
        })
        .unwrap();
        assert_eq!(config.backend.cpu_precision.as_deref(), Some("f32"));
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(config(&[("TAGGER_DEVICES", " , ")]).is_err());
        assert!(config(&[("TAGGER_CPU_THREADS", "0")]).is_err());
        assert!(config(&[("TAGGER_LISTEN", "nope")]).is_err());
        assert!(config(&[("TAGGER_CPU_PRECISION", "fp8")]).is_err());
    }
}
