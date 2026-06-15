use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::db::home_dir;
use crate::error::Result;

/// Per-model rates, all in USD per million tokens.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelRates {
    pub input: f64,
    pub output: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
    pub cache_read: f64,
}

/// Token counts for a single cost calculation.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokenCounts {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write_5m: i64,
    pub cache_write_1h: i64,
}

/// A map of model id (or prefix) to its rates, loaded from `~/.nyx/pricing.toml`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pricing {
    #[serde(flatten)]
    models: HashMap<String, ModelRates>,
}

impl Pricing {
    /// Load pricing from `~/.nyx/pricing.toml`, seeding the file with defaults
    /// first if it does not exist.
    pub fn load() -> Result<Self> {
        Self::load_from(default_pricing_path())
    }

    fn load_from(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, SEED_TOML)?;
        }
        let text = std::fs::read_to_string(&path)?;
        let pricing: Pricing =
            toml::from_str(&text).map_err(|e| crate::error::Error::Other(e.to_string()))?;
        Ok(pricing)
    }

    /// Resolve the rates for a model id: exact match first, then the longest
    /// registered prefix (so `claude-opus-4-8[1m]` resolves to
    /// `claude-opus-4-8`). Returns `None` for an unpriced model.
    fn rates_for(&self, model: &str) -> Option<&ModelRates> {
        if let Some(rates) = self.models.get(model) {
            return Some(rates);
        }
        self.models
            .iter()
            .filter(|(id, _)| model.starts_with(id.as_str()))
            .max_by_key(|(id, _)| id.len())
            .map(|(_, rates)| rates)
    }

    /// Estimated USD cost for the given token counts under the model's rates.
    /// Returns `None` for an unpriced model.
    pub fn cost_for(&self, model: &str, tokens: TokenCounts) -> Option<f64> {
        let r = self.rates_for(model)?;
        let per_mtok = |count: i64, rate: f64| (count as f64) * rate / 1_000_000.0;
        Some(
            per_mtok(tokens.input, r.input)
                + per_mtok(tokens.output, r.output)
                + per_mtok(tokens.cache_read, r.cache_read)
                + per_mtok(tokens.cache_write_5m, r.cache_write_5m)
                + per_mtok(tokens.cache_write_1h, r.cache_write_1h),
        )
    }
}

pub fn default_pricing_path() -> PathBuf {
    home_dir().join(".nyx").join("pricing.toml")
}

/// Seed pricing table written on first run. Base input/output rates are per
/// MTok; cache rates are derived from input (write-5m 1.25x, write-1h 2x,
/// read 0.1x) per the spec.
const SEED_TOML: &str = "\
[\"claude-opus-4\"]
input = 5.0
output = 25.0
cache_write_5m = 6.25
cache_write_1h = 10.0
cache_read = 0.5

[\"claude-sonnet-4-6\"]
input = 3.0
output = 15.0
cache_write_5m = 3.75
cache_write_1h = 6.0
cache_read = 0.3

[\"claude-haiku-4-5\"]
input = 1.0
output = 5.0
cache_write_5m = 1.25
cache_write_1h = 2.0
cache_read = 0.1

[\"claude-fable-5\"]
input = 10.0
output = 50.0
cache_write_5m = 12.5
cache_write_1h = 20.0
cache_read = 1.0
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_absent_writes_seed_and_prices_opus() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pricing.toml");
        assert!(!path.exists());

        let pricing = Pricing::load_from(path.clone()).unwrap();
        assert!(path.exists(), "seed file should be written when absent");

        // Known token vector: 1M input, 1M output, 1M cache-read,
        // 1M cache-write-5m, 1M cache-write-1h on Opus.
        let tokens = TokenCounts {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
        };
        let cost = pricing.cost_for("claude-opus-4-8", tokens).unwrap();
        // 5 + 25 + 0.5 + 6.25 + 10 = 46.75
        assert!((cost - 46.75).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn unknown_model_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pricing.toml");
        let pricing = Pricing::load_from(path).unwrap();
        let tokens = TokenCounts {
            input: 1_000_000,
            ..Default::default()
        };
        assert_eq!(pricing.cost_for("gpt-4", tokens), None);
    }

    #[test]
    fn one_m_variant_resolves_via_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pricing.toml");
        let pricing = Pricing::load_from(path).unwrap();
        let tokens = TokenCounts {
            input: 1_000_000,
            ..Default::default()
        };
        let cost = pricing.cost_for("claude-opus-4-8[1m]", tokens).unwrap();
        assert!((cost - 5.0).abs() < 1e-9, "got {cost}");
    }
}
