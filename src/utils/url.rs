use anyhow::{Context, Result};
use url::Url;

pub fn resolve(base: &str, href: &str) -> Result<String> {
    let base = Url::parse(base).with_context(|| format!("bad base url {base}"))?;
    let resolved = base.join(href).with_context(|| format!("bad relative url {href}"))?;
    Ok(resolved.to_string())
}
