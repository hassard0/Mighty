use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub package: Package,
    #[serde(default)]
    pub deps: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub edition: String,
    #[serde(default = "default_profile")]
    pub profile: String,
}

fn default_profile() -> String {
    "host".into()
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("read error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

pub fn load(path: &std::path::Path) -> Result<Manifest, ManifestError> {
    let src = std::fs::read_to_string(path)?;
    let m: Manifest = toml::from_str(&src)?;
    Ok(m)
}
