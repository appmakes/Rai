use anyhow::{Context, Result};
use keyring::Entry;

const SERVICE_NAME: &str = "rai";

pub fn set_api_key(provider: &str, api_key: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, provider).context("Failed to create keyring entry")?;
    entry
        .set_password(api_key)
        .context("Failed to save API key to keyring")?;
    Ok(())
}

#[cfg(not(test))]
pub fn get_api_key(provider: &str) -> Result<String> {
    let entry = Entry::new(SERVICE_NAME, provider).context("Failed to create keyring entry")?;
    let api_key = entry
        .get_password()
        .context("Failed to retrieve API key from keyring")?;
    Ok(api_key)
}

#[allow(dead_code)]
pub fn delete_api_key(provider: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, provider).context("Failed to create keyring entry")?;
    entry
        .delete_credential()
        .context("Failed to delete API key from keyring")?;
    Ok(())
}
