//! What the server keeps, as Settings › Storage shows it: its conversations'
//! databases, its attachments, its Clouds' store, its log and its search
//! indexes.
//!
//! A measurement reads every database file's size, lists every attachment and
//! asks the machine manager for its store's bytes, so one serves every page
//! for 10 minutes: Settings opened twice in a row measures once.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use demi_machine_manager_protocol::DiskUsageParams;
use demi_shared_types::Timestamp;
use tokio::sync::Mutex;

use crate::services::Services;

/// How long a measurement serves the pages that ask.
const FRESH_FOR: Duration = Duration::from_secs(10 * 60);

/// The server's one measurement and when it was taken.
#[derive(Default)]
pub struct ServerStorage {
    latest: Mutex<Option<(Instant, Arc<Measured>)>>,
}

/// What a measurement found.
pub struct Measured {
    pub at: Timestamp,
    /// The size and free bytes of the data directory's volume.
    pub disk: Option<(u64, u64)>,
    /// Each conversation's database files, by its id.
    pub conversations: HashMap<String, u64>,
    /// Each user's attachments, by the user's id.
    pub attachments: HashMap<String, u64>,
    /// The Clouds' store; none on a server without Clouds.
    pub clouds: Option<u64>,
    pub log: u64,
    pub search: u64,
}

impl ServerStorage {
    /// The latest measurement, taken again when it is older than 10 minutes.
    pub async fn measured(&self, services: &Services) -> Arc<Measured> {
        let mut latest = self.latest.lock().await;
        if let Some((taken, measured)) = latest.as_ref()
            && taken.elapsed() < FRESH_FOR
        {
            return measured.clone();
        }
        let measured = Arc::new(measure(services).await);
        *latest = Some((Instant::now(), measured.clone()));
        measured
    }
}

async fn measure(services: &Services) -> Measured {
    let data = services.data_dir.clone();
    let files = tokio::task::spawn_blocking(move || Files::read(&data)).await.unwrap_or_default();
    let attachments = services.blobs.bytes_by_user().await.unwrap_or_else(|error| {
        tracing::warn!(event = "server_storage.attachments_unread", %error, "the attachments' sizes could not be listed");
        HashMap::new()
    });
    let clouds = services.cloud.machines.call(DiskUsageParams {}).await.ok();
    Measured {
        at: services.clock.now(),
        disk: files.disk,
        conversations: files.conversations,
        attachments,
        clouds,
        log: services.log.bytes(),
        search: files.search,
    }
}

/// What the data directory's files take.
#[derive(Default)]
struct Files {
    disk: Option<(u64, u64)>,
    conversations: HashMap<String, u64>,
    search: u64,
}

impl Files {
    fn read(data: &Path) -> Self {
        let mut conversations: HashMap<String, u64> = HashMap::new();
        for (name, bytes) in sizes(&data.join(crate::services::CONVERSATION_DATABASES)) {
            // `<id>.sqlite` and its `-wal` and `-shm` beside it.
            if let Some((id, _)) = name.split_once(".sqlite") {
                *conversations.entry(id.to_owned()).or_default() += bytes;
            }
        }
        let search = sizes(&data.join(crate::services::SEARCH_INDEXES)).map(|(_, bytes)| bytes).sum();
        Self { disk: volume(data), conversations, search }
    }
}

/// The files directly in `directory` with the bytes each takes.
fn sizes(directory: &Path) -> impl Iterator<Item = (String, u64)> {
    std::fs::read_dir(directory).into_iter().flatten().flatten().filter_map(|entry| {
        let metadata = entry.metadata().ok().filter(std::fs::Metadata::is_file)?;
        Some((entry.file_name().to_string_lossy().into_owned(), metadata.len()))
    })
}

/// The size and free bytes of the volume `path` is on.
fn volume(path: &Path) -> Option<(u64, u64)> {
    let path: PathBuf = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| (disk.total_space(), disk.available_space()))
}
