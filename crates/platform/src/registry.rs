//! Small, read-mostly view of the Windows registry.

#[cfg(not(windows))]
use crate::PlatformError;
use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// A registry value the engine understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Binary(Vec<u8>),
    Number(u64),
    Other,
}

impl Value {
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{Hive, Value};
    use crate::{PlatformError, Result};
    use winreg::RegKey;
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_BINARY, REG_DWORD,
        REG_EXPAND_SZ, REG_QWORD, REG_SZ,
    };

    fn root(hive: Hive) -> RegKey {
        RegKey::predef(match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        })
    }

    fn convert(raw: &winreg::RegValue) -> Value {
        match raw.vtype {
            REG_SZ | REG_EXPAND_SZ => {
                let units: Vec<u16> = raw
                    .bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .take_while(|&u| u != 0)
                    .collect();
                Value::Text(String::from_utf16_lossy(&units))
            }
            REG_BINARY => Value::Binary(raw.bytes.to_vec()),
            REG_DWORD if raw.bytes.len() >= 4 => Value::Number(u64::from(u32::from_le_bytes([
                raw.bytes[0],
                raw.bytes[1],
                raw.bytes[2],
                raw.bytes[3],
            ]))),
            REG_QWORD if raw.bytes.len() >= 8 => {
                let mut b = [0u8; 8];
                b.copy_from_slice(&raw.bytes[..8]);
                Value::Number(u64::from_le_bytes(b))
            }
            _ => Value::Other,
        }
    }

    pub fn subkeys(hive: Hive, path: &str) -> Vec<String> {
        root(hive)
            .open_subkey_with_flags(path, KEY_READ)
            .map(|key| key.enum_keys().filter_map(|k| k.ok()).collect())
            .unwrap_or_default()
    }

    pub fn values(hive: Hive, path: &str) -> Vec<(String, Value)> {
        root(hive)
            .open_subkey_with_flags(path, KEY_READ)
            .map(|key| {
                key.enum_values()
                    .filter_map(|v| v.ok())
                    .map(|(name, raw)| (name, convert(&raw)))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn value(hive: Hive, path: &str, name: &str) -> Option<Value> {
        let key = root(hive).open_subkey_with_flags(path, KEY_READ).ok()?;
        key.get_raw_value(name).ok().map(|raw| convert(&raw))
    }

    pub fn set_binary(hive: Hive, path: &str, name: &str, bytes: &[u8]) -> Result<()> {
        let (key, _) = root(hive)
            .create_subkey_with_flags(path, KEY_SET_VALUE)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    PlatformError::NeedsAdmin
                } else {
                    PlatformError::Io {
                        path: path.into(),
                        source: e,
                    }
                }
            })?;
        let raw = winreg::RegValue {
            bytes: bytes.to_vec().into(),
            vtype: REG_BINARY,
        };
        key.set_raw_value(name, &raw).map_err(|e| {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                PlatformError::NeedsAdmin
            } else {
                PlatformError::Io {
                    path: path.into(),
                    source: e,
                }
            }
        })
    }
}

/// Names of the subkeys of `path`; empty if it does not exist or cannot be read.
pub fn subkeys(hive: Hive, path: &str) -> Vec<String> {
    #[cfg(windows)]
    return imp::subkeys(hive, path);
    #[cfg(not(windows))]
    {
        let _ = (hive, path);
        Vec::new()
    }
}

/// All values of `path`; empty if it does not exist or cannot be read.
pub fn values(hive: Hive, path: &str) -> Vec<(String, Value)> {
    #[cfg(windows)]
    return imp::values(hive, path);
    #[cfg(not(windows))]
    {
        let _ = (hive, path);
        Vec::new()
    }
}

/// One value, if present.
pub fn value(hive: Hive, path: &str, name: &str) -> Option<Value> {
    #[cfg(windows)]
    return imp::value(hive, path, name);
    #[cfg(not(windows))]
    {
        let _ = (hive, path, name);
        None
    }
}

/// Writes a `REG_BINARY` value, creating the key if needed.
pub fn set_binary(hive: Hive, path: &str, name: &str, bytes: &[u8]) -> Result<()> {
    #[cfg(windows)]
    return imp::set_binary(hive, path, name, bytes);
    #[cfg(not(windows))]
    {
        let _ = (hive, path, name, bytes);
        Err(PlatformError::Unsupported("registry"))
    }
}
