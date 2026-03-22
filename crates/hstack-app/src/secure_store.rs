use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

#[cfg(not(mobile))]
use keyring::Entry;
#[cfg(mobile)]
use tauri_plugin_store::StoreExt;

static KEY_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
#[cfg(mobile)]
const MOBILE_STORE_PATH: &str = "secure_store.json";
#[cfg(mobile)]
const MOBILE_STORE_KEY: &str = "entries";

fn get_cache() -> &'static Mutex<HashMap<String, String>> {
    KEY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct SecureStore;

impl SecureStore {
    const SERVICE_NAME: &'static str = "hstack-llm-service";

    #[cfg(mobile)]
    fn mobile_entries(app: &AppHandle) -> Result<HashMap<String, String>, String> {
        let store = app
            .store(MOBILE_STORE_PATH)
            .map_err(|e| format!("Secure store open failure: {}", e))?;

        match store.get(MOBILE_STORE_KEY) {
            Some(value) => serde_json::from_value(value)
                .map_err(|e| format!("Secure store parse failure: {}", e)),
            None => Ok(HashMap::new()),
        }
    }

    #[cfg(mobile)]
    fn save_mobile_entries(app: &AppHandle, entries: HashMap<String, String>) -> Result<(), String> {
        let store = app
            .store(MOBILE_STORE_PATH)
            .map_err(|e| format!("Secure store open failure: {}", e))?;

        store.set(MOBILE_STORE_KEY, serde_json::json!(entries));
        store
            .save()
            .map_err(|e| format!("Secure store save failure: {}", e))
    }

    pub fn set_key(_app: &AppHandle, id: &str, key: &str) -> Result<(), String> {
        #[cfg(not(mobile))]
        {
        let entry = Entry::new(Self::SERVICE_NAME, id)
            .map_err(|e| format!("OS Keychain access error: {}", e))?;
        
        entry.set_password(key)
            .map_err(|e| format!("Failed to save secret to OS Keychain: {}", e))?;
        }

        #[cfg(mobile)]
        {
            let mut entries = Self::mobile_entries(_app)?;
            entries.insert(id.to_string(), key.to_string());
            Self::save_mobile_entries(_app, entries)?;
        }
            
        if let Ok(mut cache) = get_cache().lock() {
            cache.insert(id.to_string(), key.to_string());
        }
        Ok(())
    }

    pub fn get_key(_app: &AppHandle, id: &str) -> Result<String, String> {
        if let Ok(cache) = get_cache().lock() {
            if let Some(key) = cache.get(id) {
                return Ok(key.clone());
            }
        }

        #[cfg(not(mobile))]
        {
        let entry = Entry::new(Self::SERVICE_NAME, id)
            .map_err(|e| format!("OS Keychain access error: {}", e))?;

            return match entry.get_password() {
            Ok(p) => {
                if let Ok(mut cache) = get_cache().lock() {
                    cache.insert(id.to_string(), p.clone());
                }
                Ok(p)
            },
            Err(keyring::Error::NoEntry) => Ok("".to_string()),
            Err(e) => Err(format!("Failed to retrieve secret from OS Keychain: {}", e)),
            };
        }

        #[cfg(mobile)]
        {
            let entries = Self::mobile_entries(_app)?;
            let value = entries.get(id).cloned().unwrap_or_default();

            if let Ok(mut cache) = get_cache().lock() {
                cache.insert(id.to_string(), value.clone());
            }

            Ok(value)
        }
    }

    pub fn delete_key(_app: &AppHandle, id: &str) -> Result<(), String> {
        #[cfg(not(mobile))]
        {
        let entry = Entry::new(Self::SERVICE_NAME, id)
            .map_err(|e| format!("OS Keychain access error: {}", e))?;

            match entry.delete_credential() {
                Ok(_) | Err(keyring::Error::NoEntry) => {}
                Err(e) => return Err(format!("Failed to delete secret from OS Keychain: {}", e)),
            }
        }

        #[cfg(mobile)]
        {
            let mut entries = Self::mobile_entries(_app)?;
            entries.remove(id);
            Self::save_mobile_entries(_app, entries)?;
        }

        if let Ok(mut cache) = get_cache().lock() {
            cache.remove(id);
        }

        Ok(())
    }
}
