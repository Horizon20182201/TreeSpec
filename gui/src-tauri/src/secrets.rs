//! The LLM API key, kept only in the macOS Keychain (design §9.3): a generic
//! password item with the bundle ID as service and `llm-api-key` as account.
//! It is read right before a request and never sent to the WebView, logs,
//! diagnostics or equivalent commands.

use std::sync::Mutex;

pub const ACCOUNT: &str = "llm-api-key";

pub trait SecretStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, String>;
    /// `None` removes the key.
    fn set(&self, key: Option<&str>) -> Result<(), String>;
}

/// The macOS Keychain.
pub struct Keychain;

#[cfg(target_os = "macos")]
impl SecretStore for Keychain {
    fn get(&self) -> Result<Option<String>, String> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(crate::BUNDLE_ID, ACCOUNT) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "钥匙串中的密钥不是 UTF-8 文本".into()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(format!("无法读取钥匙串：{e}")),
        }
    }

    fn set(&self, key: Option<&str>) -> Result<(), String> {
        use security_framework::passwords::{delete_generic_password, set_generic_password};
        match key {
            Some(key) => set_generic_password(crate::BUNDLE_ID, ACCOUNT, key.as_bytes())
                .map_err(|e| format!("无法写入钥匙串：{e}")),
            None => match delete_generic_password(crate::BUNDLE_ID, ACCOUNT) {
                Err(e) if e.code() != ERR_SEC_ITEM_NOT_FOUND => {
                    Err(format!("无法删除钥匙串项：{e}"))
                }
                _ => Ok(()),
            },
        }
    }
}

#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(not(target_os = "macos"))]
impl SecretStore for Keychain {
    fn get(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn set(&self, _: Option<&str>) -> Result<(), String> {
        Err("钥匙串只在 macOS 上可用".into())
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct Memory(pub Mutex<Option<String>>);

impl SecretStore for Memory {
    fn get(&self) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn set(&self, key: Option<&str>) -> Result<(), String> {
        *self.0.lock().unwrap() = key.map(str::to_owned);
        Ok(())
    }
}
