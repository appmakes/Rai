use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const SERVICE_NAME: &str = "rai";

fn credentials_path() -> Result<PathBuf> {
    #[cfg(unix)]
    {
        let home = std::env::var("HOME").context("HOME not set")?;
        Ok(PathBuf::from(home).join(".local/share/rai/credentials"))
    }
    #[cfg(not(unix))]
    {
        let dirs = directories::ProjectDirs::from("com", "rai", "rai")
            .context("Failed to determine credentials directory")?;
        Ok(dirs.data_dir().join("credentials"))
    }
}

fn read_credentials_file() -> Result<HashMap<String, String>> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let content = fs::read_to_string(&path).context("Failed to read credentials file")?;
    let map: HashMap<String, String> =
        serde_json::from_str(&content).unwrap_or_default();
    Ok(map)
}

fn write_credentials_file(map: &HashMap<String, String>) -> Result<()> {
    let path = credentials_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create credentials directory")?;
    }
    let content =
        serde_json::to_string(map).context("Failed to serialize credentials file")?;
    fs::write(&path, content).context("Failed to write credentials file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path).context("Failed to read credentials file metadata")?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&path, perms).context("Failed to set credentials file permissions")?;
    }
    Ok(())
}

pub fn set_api_key(account: &str, api_key: &str, use_keyring: bool) -> Result<()> {
    if use_keyring {
        #[cfg(not(test))]
        {
            use keyring::Entry;
            let entry = Entry::new(SERVICE_NAME, account).context("Failed to create keyring entry")?;
            entry
                .set_password(api_key)
                .context("Failed to save API key to keyring")?;
        }
        #[cfg(test)]
        let _ = (account, api_key);
        return Ok(());
    }
    let mut map = read_credentials_file()?;
    map.insert(account.to_string(), api_key.to_string());
    write_credentials_file(&map)
}

pub fn get_api_key(account: &str, use_keyring: bool) -> Result<String> {
    if use_keyring {
        #[cfg(not(test))]
        {
            use keyring::Entry;
            let entry = Entry::new(SERVICE_NAME, account).context("Failed to create keyring entry")?;
            let api_key = entry
                .get_password()
                .context("Failed to retrieve API key from keyring")?;
            return Ok(api_key);
        }
        #[cfg(test)]
        return Err(anyhow::anyhow!("keyring disabled in test"));
    }
    let map = read_credentials_file()?;
    map.get(account)
        .cloned()
        .context("No API key found for account")
}

#[allow(dead_code)]
pub fn delete_api_key(account: &str, use_keyring: bool) -> Result<()> {
    if use_keyring {
        #[cfg(not(test))]
        {
            use keyring::Entry;
            let entry = Entry::new(SERVICE_NAME, account).context("Failed to create keyring entry")?;
            entry
                .delete_credential()
                .context("Failed to delete API key from keyring")?;
        }
        return Ok(());
    }
    let mut map = read_credentials_file()?;
    map.remove(account);
    write_credentials_file(&map)
}
