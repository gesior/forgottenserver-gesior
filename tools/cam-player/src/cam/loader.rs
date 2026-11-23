use super::parser::{CamParser, ParsedCam};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

struct DiskEntry {
    size: u64,
    last_used: SystemTime,
    rel_path: Option<String>,
}

struct DiskCache {
    entries: HashMap<String, DiskEntry>,
    total_bytes: u64,
}

pub struct CamLoader {
    cams_dir: PathBuf,
    cache_dir: PathBuf,
    max_disk_cache_bytes: u64,
    live: Mutex<HashMap<String, Weak<ParsedCam>>>,
    disk: Mutex<DiskCache>,
}

impl CamLoader {
    pub fn new<P1: AsRef<Path>, P2: AsRef<Path>>(
        cams_dir: P1,
        cache_dir: P2,
        max_disk_cache_bytes: u64,
    ) -> Self {
        let cams_path = cams_dir.as_ref().to_path_buf();
        let cache_path = cache_dir.as_ref().to_path_buf();
        let _ = fs::create_dir_all(&cache_path);
        let mut disk = scan_cache_dir(&cache_path);
        evict_until(&mut disk, &cache_path, max_disk_cache_bytes, &HashMap::new(), None);

        Self {
            cams_dir: cams_path,
            cache_dir: cache_path,
            max_disk_cache_bytes,
            live: Mutex::new(HashMap::new()),
            disk: Mutex::new(disk),
        }
    }

    pub async fn load_cam(
        self: &Arc<Self>,
        rel_path: &str,
    ) -> Result<Arc<ParsedCam>, Box<dyn std::error::Error + Send + Sync>> {
        let loader = Arc::clone(self);
        let rel_path = rel_path.to_string();
        match tokio::task::spawn_blocking(move || loader.load_blocking(&rel_path)).await {
            Ok(result) => result,
            Err(err) => Err(format!("Cam load task failed: {err}").into()),
        }
    }

    fn load_blocking(
        &self,
        rel_path: &str,
    ) -> Result<Arc<ParsedCam>, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(existing) = self.upgrade_live(rel_path) {
            self.touch_disk(rel_path);
            return Ok(existing);
        }

        let cache_name = cache_file_name(rel_path);
        let cache_file = self.cache_dir.join(&cache_name);
        let from_disk = cache_file.is_file();

        let bytes = if from_disk {
            fs::read(&cache_file)?
        } else {
            let archive = self.archive_path(rel_path)?;
            if !archive.is_file() {
                return Err(format!("Archive file not found: {}", archive.display()).into());
            }
            self.decompress_7z(&archive)?
        };

        let arc = Arc::new(CamParser::parse(&bytes)?);

        {
            let mut live = lock_mutex(&self.live);
            if let Some(existing) = live.get(rel_path).and_then(Weak::upgrade) {
                drop(live);
                self.touch_disk(rel_path);
                return Ok(existing);
            }
            live.retain(|_, weak| weak.strong_count() > 0);
            live.insert(rel_path.to_string(), Arc::downgrade(&arc));
        }

        if from_disk {
            self.note_disk_hit(rel_path, &cache_name, bytes.len() as u64);
        } else {
            self.try_store(rel_path, &cache_name, &bytes);
        }
        Ok(arc)
    }

    fn upgrade_live(&self, rel_path: &str) -> Option<Arc<ParsedCam>> {
        let mut live = lock_mutex(&self.live);
        let upgraded = live.get(rel_path).and_then(Weak::upgrade);
        if upgraded.is_none() {
            live.remove(rel_path);
        }
        upgraded
    }

    fn archive_path(&self, rel_path: &str) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        if rel_path.is_empty() || rel_path.contains('\0') {
            return Err("Invalid cam path".into());
        }
        let rel = Path::new(rel_path);
        if rel.is_absolute() || rel.components().any(|component| matches!(component, Component::ParentDir)) {
            return Err("Invalid cam path".into());
        }
        Ok(self.cams_dir.join(rel))
    }

    fn touch_disk(&self, rel_path: &str) {
        let cache_name = cache_file_name(rel_path);
        let cache_file = self.cache_dir.join(&cache_name);
        let mut disk = lock_mutex(&self.disk);
        let Some(entry) = disk.entries.get_mut(&cache_name) else {
            return;
        };
        let now = SystemTime::now();
        entry.last_used = now;
        entry.rel_path = Some(rel_path.to_string());
        drop(disk);
        let _ = touch_mtime(&cache_file, now);
    }

    fn note_disk_hit(&self, rel_path: &str, cache_name: &str, size: u64) {
        let now = SystemTime::now();
        {
        let mut disk = lock_mutex(&self.disk);
        let previous_size = disk.entries.get(cache_name).map(|entry| entry.size);
        match previous_size {
            Some(previous) if previous != size => {
                disk.total_bytes = disk.total_bytes.saturating_sub(previous).saturating_add(size);
            }
            None => {
                disk.total_bytes = disk.total_bytes.saturating_add(size);
            }
            _ => {}
        }
        let entry = disk.entries.entry(cache_name.to_string()).or_insert(DiskEntry {
            size,
            last_used: now,
            rel_path: None,
        });
        entry.size = size;
        entry.last_used = now;
        entry.rel_path = Some(rel_path.to_string());
        }
        let _ = touch_mtime(&self.cache_dir.join(cache_name), now);
    }

    fn try_store(&self, rel_path: &str, cache_name: &str, bytes: &[u8]) {
        let size = bytes.len() as u64;
        if size == 0 || size > self.max_disk_cache_bytes {
            return;
        }

        let live = lock_mutex(&self.live);
        let mut disk = lock_mutex(&self.disk);
        let already = disk.entries.get(cache_name).map(|entry| entry.size).unwrap_or(0);
        let growth = size.saturating_sub(already);
        if !reserve_space(&mut disk, &self.cache_dir, self.max_disk_cache_bytes, growth, &live, Some(cache_name)) {
            return;
        }
        drop(live);

        let partial = self.cache_dir.join(format!("{cache_name}.partial"));
        let final_path = self.cache_dir.join(cache_name);
        if fs::write(&partial, bytes).is_err() {
            let _ = fs::remove_file(&partial);
            return;
        }
        if fs::rename(&partial, &final_path).is_err() {
            let _ = fs::remove_file(&partial);
            return;
        }

        let now = SystemTime::now();
        let _ = touch_mtime(&final_path, now);
        if let Some(previous) = disk.entries.insert(
            cache_name.to_string(),
            DiskEntry {
                size,
                last_used: now,
                rel_path: Some(rel_path.to_string()),
            },
        ) {
            disk.total_bytes = disk.total_bytes.saturating_sub(previous.size);
        }
        disk.total_bytes = disk.total_bytes.saturating_add(size);
    }

    fn decompress_7z(&self, archive: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let temp_dir = self.cache_dir.join(format!("tmp_{}_{seq}", std::process::id()));
        fs::create_dir_all(&temp_dir)?;

        let extracted = (|| -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
            if sevenz_rust::decompress_file(archive, &temp_dir).is_ok() {
                if let Some(path) = first_file(&temp_dir) {
                    return Ok(fs::read(path)?);
                }
            }

            let output = Command::new("7z").args(["x", "-so"]).arg(archive).output();
            if let Ok(output) = output {
                if output.status.success() && !output.stdout.is_empty() {
                    return Ok(output.stdout);
                }
            }
            Err(format!("Failed to decompress 7z archive: {}", archive.display()).into())
        })();

        let _ = fs::remove_dir_all(&temp_dir);
        extracted
    }
}

fn first_file(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                return Some(path);
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    None
}

fn cache_file_name(rel_path: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(rel_path.as_bytes());
    format!("{:x}.cam", hasher.finalize())
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn touch_mtime(path: &Path, when: SystemTime) -> std::io::Result<()> {
    let file = OpenOptions::new().write(true).open(path)?;
    file.set_modified(when)
}

fn scan_cache_dir(cache_dir: &Path) -> DiskCache {
    let mut disk = DiskCache {
        entries: HashMap::new(),
        total_bytes: 0,
    };
    let Ok(entries) = fs::read_dir(cache_dir) else {
        return disk;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() && name.starts_with("tmp_") {
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        if path.is_file() && (name.ends_with(".partial") || name.ends_with(".tmp")) {
            let _ = fs::remove_file(&path);
            continue;
        }
        if path.is_file() && name.ends_with(".cam") {
            if let Ok(meta) = entry.metadata() {
                let size = meta.len();
                disk.total_bytes = disk.total_bytes.saturating_add(size);
                disk.entries.insert(
                    name,
                    DiskEntry {
                        size,
                        last_used: meta.modified().unwrap_or(UNIX_EPOCH),
                        rel_path: None,
                    },
                );
            }
        }
    }
    disk
}

fn evict_until(
    disk: &mut DiskCache,
    cache_dir: &Path,
    max_bytes: u64,
    live: &HashMap<String, Weak<ParsedCam>>,
    keep_file: Option<&str>,
) {
    while disk.total_bytes > max_bytes {
        let Some(victim) = pick_victim(&disk.entries, live, keep_file) else {
            break;
        };
        remove_cache_file(disk, cache_dir, &victim);
    }
}

fn reserve_space(
    disk: &mut DiskCache,
    cache_dir: &Path,
    max_bytes: u64,
    additional: u64,
    live: &HashMap<String, Weak<ParsedCam>>,
    keep_file: Option<&str>,
) -> bool {
    if additional > max_bytes {
        return false;
    }
    let target = max_bytes - additional;
    evict_until(disk, cache_dir, target, live, keep_file);
    disk.total_bytes <= target
}

fn pick_victim(
    entries: &HashMap<String, DiskEntry>,
    live: &HashMap<String, Weak<ParsedCam>>,
    keep_file: Option<&str>,
) -> Option<String> {
    entries
        .iter()
        .filter(|(name, entry)| {
            if Some(name.as_str()) == keep_file {
                return false;
            }
            !entry_in_use(entry, live)
        })
        .min_by_key(|(_, entry)| entry.last_used)
        .map(|(name, _)| name.clone())
}

fn entry_in_use(entry: &DiskEntry, live: &HashMap<String, Weak<ParsedCam>>) -> bool {
    entry
        .rel_path
        .as_ref()
        .and_then(|rel_path| live.get(rel_path))
        .is_some_and(|weak| weak.strong_count() > 0)
}

fn remove_cache_file(disk: &mut DiskCache, cache_dir: &Path, name: &str) {
    if let Some(entry) = disk.entries.remove(name) {
        disk.total_bytes = disk.total_bytes.saturating_sub(entry.size);
        let _ = fs::remove_file(cache_dir.join(name));
        println!("[CamPlayer] Evicted disk cache {name} ({} bytes)", entry.size);
    }
}

#[cfg(test)]
mod tests {
    use super::{pick_victim, DiskEntry};
    use crate::cam::parser::ParsedCam;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::UNIX_EPOCH;

    fn entry(size: u64, secs: u64, rel_path: Option<&str>) -> DiskEntry {
        DiskEntry {
            size,
            last_used: UNIX_EPOCH + std::time::Duration::from_secs(secs),
            rel_path: rel_path.map(str::to_string),
        }
    }

    fn dummy_cam() -> ParsedCam {
        ParsedCam {
            player_id: 1,
            character_name: String::new(),
            protocol_version: 1098,
            start_time_ms: 0,
            first_packet_time_ms: 0,
            last_packet_time_ms: 0,
            duration_ms: 0,
            packets: Vec::new(),
            sync_snapshots: Vec::new(),
        }
    }

    #[test]
    fn evicts_the_least_recently_played_idle_file() {
        let mut entries = HashMap::new();
        entries.insert("old.cam".to_string(), entry(10, 1, Some("old")));
        entries.insert("new.cam".to_string(), entry(10, 50, Some("new")));
        entries.insert("mid.cam".to_string(), entry(10, 20, None));

        let live = HashMap::new();
        assert_eq!(pick_victim(&entries, &live, None).as_deref(), Some("old.cam"));
    }

    #[test]
    fn keeps_a_cam_that_is_currently_playing() {
        let mut entries = HashMap::new();
        entries.insert("old.cam".to_string(), entry(10, 1, Some("old")));
        entries.insert("newer.cam".to_string(), entry(10, 5, Some("newer")));

        let arc = Arc::new(dummy_cam());
        let mut live = HashMap::new();
        live.insert("old".to_string(), Arc::downgrade(&arc));

        assert_eq!(pick_victim(&entries, &live, None).as_deref(), Some("newer.cam"));
    }

    #[test]
    fn does_not_evict_the_file_being_written() {
        let mut entries = HashMap::new();
        entries.insert("keep.cam".to_string(), entry(10, 1, None));
        entries.insert("other.cam".to_string(), entry(10, 2, None));

        let live = HashMap::new();
        assert_eq!(pick_victim(&entries, &live, Some("keep.cam")).as_deref(), Some("other.cam"));
    }
}
