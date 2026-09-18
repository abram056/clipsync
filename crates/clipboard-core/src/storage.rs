use std::sync::Mutex;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use clipboard_proto::error::{Error, Result};
use clipboard_proto::types::{HistoryEntry, TrustedPeer};

use crate::config::StorageConfig;

const CURRENT_SCHEMA_VERSION: u32 = 1;
const HISTORY_CAP_BYTES: u64 = 2_097_152;

pub struct Storage {
    conn: Mutex<Connection>,
}

impl Storage {
    pub fn open(config: &StorageConfig) -> Result<Self> {
        let path = &config.path;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::Storage(format!("failed to create data dir: {}", e)))?;
        }
        let conn = Connection::open(path)
            .map_err(|e| Error::Storage(format!("failed to open database: {}", e)))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| Error::Storage(format!("PRAGMA failed: {}", e)))?;
        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap_or(0);

        if version < 1 {
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS device_config (
                    id          INTEGER PRIMARY KEY CHECK (id = 1),
                    device_id   TEXT NOT NULL,
                    device_name TEXT NOT NULL,
                    created_at  TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS trusted_peers (
                    device_id   TEXT PRIMARY KEY NOT NULL,
                    device_name TEXT NOT NULL,
                    platform    TEXT,
                    paired_at   TEXT NOT NULL,
                    last_seen   TEXT
                );

                CREATE TABLE IF NOT EXISTS clipboard_history (
                    id               INTEGER PRIMARY KEY AUTOINCREMENT,
                    clipboard_id     TEXT NOT NULL UNIQUE,
                    content          TEXT NOT NULL,
                    content_type     TEXT NOT NULL DEFAULT 'text/plain',
                    content_hash     TEXT NOT NULL,
                    origin_device_id TEXT NOT NULL,
                    created_at       TEXT NOT NULL,
                    size_bytes       INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_history_created ON clipboard_history (created_at DESC);
                CREATE INDEX IF NOT EXISTS idx_history_hash ON clipboard_history (content_hash);

                CREATE TABLE IF NOT EXISTS app_settings (
                    key   TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL
                );
                ",
            )
            .map_err(|e| Error::Storage(format!("migration failed: {}", e)))?;
            conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
                .map_err(|e| Error::Storage(format!("version update failed: {}", e)))?;
        }
        Ok(())
    }

    // --- Device Identity ---

    pub fn get_device_identity(&self) -> Result<(Uuid, String, DateTime<Utc>)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT device_id, device_name, created_at FROM device_config WHERE id = 1")
            .map_err(|e| Error::Storage(e.to_string()))?;
        let result = stmt.query_row([], |row| {
            let id_str: String = row.get(0)?;
            let name: String = row.get(1)?;
            let created: String = row.get(2)?;
            Ok((id_str, name, created))
        });

        match result {
            Ok((id_str, name, created)) => {
                let id = Uuid::parse_str(&id_str)
                    .map_err(|e| Error::Storage(format!("invalid device_id: {}", e)))?;
                let created_at = DateTime::parse_from_rfc3339(&created)
                    .map_err(|e| Error::Storage(format!("invalid created_at: {}", e)))?
                    .with_timezone(&Utc);
                Ok((id, name, created_at))
            }
            Err(_) => {
                let id = Uuid::new_v4();
                let name = gethostname::gethostname().to_string_lossy().to_string();
                let now = Utc::now();
                conn.execute(
                    "INSERT INTO device_config (id, device_id, device_name, created_at) VALUES (1, ?1, ?2, ?3)",
                    params![id.to_string(), name, now.to_rfc3339()],
                )
                .map_err(|e| Error::Storage(e.to_string()))?;
                Ok((id, name, now))
            }
        }
    }

    pub fn set_device_name(&self, name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE device_config SET device_name = ?1 WHERE id = 1",
            params![name],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    // --- Trusted Peers ---

    pub fn is_trusted(&self, device_id: &Uuid) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM trusted_peers WHERE device_id = ?1",
                params![device_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(count > 0)
    }

    pub fn add_trusted_peer(&self, peer: &TrustedPeer) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO trusted_peers (device_id, device_name, platform, paired_at, last_seen) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                peer.device_id.to_string(),
                peer.device_name,
                peer.platform.as_ref().map(|p| p.to_string()),
                peer.paired_at.to_rfc3339(),
                peer.last_seen.map(|t| t.to_rfc3339()),
            ],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove_trusted_peer(&self, device_id: &Uuid) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM trusted_peers WHERE device_id = ?1",
            params![device_id.to_string()],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn trusted_peers(&self) -> Result<Vec<TrustedPeer>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT device_id, device_name, platform, paired_at, last_seen FROM trusted_peers",
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let name: String = row.get(1)?;
                let platform: Option<String> = row.get(2)?;
                let paired: String = row.get(3)?;
                let last_seen: Option<String> = row.get(4)?;
                Ok((id_str, name, platform, paired, last_seen))
            })
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut peers = Vec::new();
        for row in rows {
            let (id_str, name, platform, paired, last_seen) =
                row.map_err(|e| Error::Storage(e.to_string()))?;
            let device_id = Uuid::parse_str(&id_str)
                .map_err(|e| Error::Storage(format!("invalid device_id: {}", e)))?;
            let paired_at = DateTime::parse_from_rfc3339(&paired)
                .map_err(|e| Error::Storage(format!("invalid paired_at: {}", e)))?
                .with_timezone(&Utc);
            let last_seen = last_seen.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });
            let platform = platform.and_then(|p| match p.as_str() {
                "linux" => Some(clipboard_proto::types::Platform::Linux),
                "android" => Some(clipboard_proto::types::Platform::Android),
                "windows" => Some(clipboard_proto::types::Platform::Windows),
                "macos" => Some(clipboard_proto::types::Platform::MacOS),
                _ => None,
            });
            peers.push(TrustedPeer {
                device_id,
                device_name: name,
                platform,
                paired_at,
                last_seen,
            });
        }
        Ok(peers)
    }

    // --- Clipboard History ---

    pub fn append_history(&self, entry: &HistoryEntry) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO clipboard_history (clipboard_id, content, content_type, content_hash, origin_device_id, created_at, size_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.clipboard_id.to_string(),
                entry.content,
                entry.content_type,
                entry.content_hash,
                entry.origin_device_id.to_string(),
                entry.created_at.to_rfc3339(),
                entry.size_bytes as i64,
            ],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        self.evict_history(&conn, HISTORY_CAP_BYTES)?;
        Ok(())
    }

    pub fn history_size_bytes(&self) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let total: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(size_bytes), 0) FROM clipboard_history",
                [],
                |row| row.get(0),
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(total as u64)
    }

    pub fn list_history(&self, limit: usize) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT clipboard_id, content, content_type, content_hash, origin_device_id, created_at, size_bytes FROM clipboard_history ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| Error::Storage(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                let cid: String = row.get(0)?;
                let content: String = row.get(1)?;
                let ct: String = row.get(2)?;
                let hash: String = row.get(3)?;
                let origin: String = row.get(4)?;
                let created: String = row.get(5)?;
                let size: i64 = row.get(6)?;
                Ok((cid, content, ct, hash, origin, created, size))
            })
            .map_err(|e| Error::Storage(e.to_string()))?;

        let mut entries = Vec::new();
        for row in rows {
            let (cid, content, ct, hash, origin, created, size) =
                row.map_err(|e| Error::Storage(e.to_string()))?;
            let clipboard_id = Uuid::parse_str(&cid)
                .map_err(|e| Error::Storage(format!("invalid clipboard_id: {}", e)))?;
            let origin_device_id = Uuid::parse_str(&origin)
                .map_err(|e| Error::Storage(format!("invalid origin_device_id: {}", e)))?;
            let created_at = DateTime::parse_from_rfc3339(&created)
                .map_err(|e| Error::Storage(format!("invalid created_at: {}", e)))?
                .with_timezone(&Utc);
            entries.push(HistoryEntry {
                clipboard_id,
                content,
                content_type: ct,
                content_hash: hash,
                origin_device_id,
                created_at,
                size_bytes: size as u64,
            });
        }
        Ok(entries)
    }

    fn evict_history(&self, conn: &Connection, max_bytes: u64) -> Result<()> {
        loop {
            let total: i64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(size_bytes), 0) FROM clipboard_history",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| Error::Storage(e.to_string()))?;
            if (total as u64) <= max_bytes {
                break;
            }
            let deleted = conn
                .execute(
                    "DELETE FROM clipboard_history WHERE id IN (SELECT id FROM clipboard_history ORDER BY id ASC LIMIT 1)",
                    [],
                )
                .map_err(|e| Error::Storage(e.to_string()))?;
            if deleted == 0 {
                break;
            }
        }
        Ok(())
    }

    // --- Settings ---

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );
        match result {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Storage(e.to_string())),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| Error::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_storage() -> Storage {
        let config = StorageConfig {
            path: PathBuf::from(":memory:"),
        };
        Storage::open(&config).unwrap()
    }

    #[test]
    fn migration_is_idempotent() {
        let storage = test_storage();
        storage.migrate().unwrap();
        storage.migrate().unwrap();
    }

    #[test]
    fn device_identity_created_once() {
        let storage = test_storage();
        let (id1, name1, _) = storage.get_device_identity().unwrap();
        let (id2, name2, _) = storage.get_device_identity().unwrap();
        assert_eq!(id1, id2);
        assert_eq!(name1, name2);
    }

    #[test]
    fn set_device_name() {
        let storage = test_storage();
        storage.get_device_identity().unwrap();
        storage.set_device_name("my-phone").unwrap();
        let (_, name, _) = storage.get_device_identity().unwrap();
        assert_eq!(name, "my-phone");
    }

    #[test]
    fn trusted_peer_lifecycle() {
        let storage = test_storage();
        let id = Uuid::new_v4();
        let peer = TrustedPeer {
            device_id: id,
            device_name: "phone".to_string(),
            platform: Some(clipboard_proto::types::Platform::Android),
            paired_at: Utc::now(),
            last_seen: None,
        };
        assert!(!storage.is_trusted(&id).unwrap());
        storage.add_trusted_peer(&peer).unwrap();
        assert!(storage.is_trusted(&id).unwrap());
        let peers = storage.trusted_peers().unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_name, "phone");
        storage.remove_trusted_peer(&id).unwrap();
        assert!(!storage.is_trusted(&id).unwrap());
    }

    #[test]
    fn history_eviction() {
        let storage = test_storage();
        let entry = HistoryEntry {
            clipboard_id: Uuid::new_v4(),
            content: "a".repeat(100),
            content_type: "text/plain".to_string(),
            content_hash: "hash1".to_string(),
            origin_device_id: Uuid::new_v4(),
            created_at: Utc::now(),
            size_bytes: 100,
        };
        // Insert enough entries to exceed a small cap
        for i in 0..30 {
            let mut e = entry.clone();
            e.clipboard_id = Uuid::new_v4();
            e.content_hash = format!("hash{}", i);
            e.size_bytes = 100;
            e.content = "a".repeat(100);
            storage.append_history(&e).unwrap();
        }
        // All 30 entries fit under the 2MB cap
        let total = storage.history_size_bytes().unwrap();
        assert_eq!(total, 3000);
        // Now test eviction directly with a small cap
        {
            let conn = storage.conn.lock().unwrap();
            storage.evict_history(&conn, 1000).unwrap();
        }
        let total = storage.history_size_bytes().unwrap();
        assert!(total <= 1000, "history size {} should be <= 1000", total);
    }

    #[test]
    fn settings_crud() {
        let storage = test_storage();
        assert_eq!(storage.get_setting("foo").unwrap(), None);
        storage.set_setting("foo", "bar").unwrap();
        assert_eq!(storage.get_setting("foo").unwrap(), Some("bar".to_string()));
        storage.set_setting("foo", "baz").unwrap();
        assert_eq!(storage.get_setting("foo").unwrap(), Some("baz".to_string()));
    }
}
