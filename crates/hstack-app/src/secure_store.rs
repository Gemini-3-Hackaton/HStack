use keyring::Entry;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static KEY_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn get_cache() -> &'static Mutex<HashMap<String, String>> {
    KEY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct SecureStore;

impl SecureStore {
    const SERVICE_NAME: &'static str = "hstack-llm-service";

    pub fn set_key(id: &str, key: &str) -> Result<(), String> {
        let entry = Entry::new(Self::SERVICE_NAME, id)
            .map_err(|e| format!("OS Keychain access error: {}", e))?;
        
        entry.set_password(key)
            .map_err(|e| format!("Failed to save secret to OS Keychain: {}", e))?;
            
        if let Ok(mut cache) = get_cache().lock() {
            cache.insert(id.to_string(), key.to_string());
        }
        Ok(())
    }

    pub fn get_key(id: &str) -> Result<String, String> {
        if let Ok(cache) = get_cache().lock() {
            if let Some(key) = cache.get(id) {
                return Ok(key.clone());
            }
        }

        let entry = Entry::new(Self::SERVICE_NAME, id)
            .map_err(|e| format!("OS Keychain access error: {}", e))?;

        match entry.get_password() {
            Ok(p) => {
                if let Ok(mut cache) = get_cache().lock() {
                    cache.insert(id.to_string(), p.clone());
                }
                Ok(p)
            },
            Err(keyring::Error::NoEntry) => Ok("".to_string()),
            Err(e) => Err(format!("Failed to retrieve secret from OS Keychain: {}", e)),
        }
    }

    pub fn delete_key(id: &str) -> Result<(), String> {
        let entry = Entry::new(Self::SERVICE_NAME, id)
            .map_err(|e| format!("OS Keychain access error: {}", e))?;

        if let Ok(mut cache) = get_cache().lock() {
            cache.remove(id);
        }

        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Failed to delete secret from OS Keychain: {}", e)),
        }
    }
}
