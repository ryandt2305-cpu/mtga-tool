// ImageCacheService — downloads card images to disk, serves via Tauri asset protocol.
// Zero network requests for cached images after initial sync.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tauri_plugin_http::reqwest;
use tokio::sync::Semaphore;

use crate::events;

/// Concurrent download limit — 8 parallel requests to Scryfall CDN.
const CONCURRENCY: usize = 8;
/// Emit progress event every N completed downloads.
const PROGRESS_INTERVAL: usize = 25;
/// Maximum retry attempts per download.
const MAX_RETRIES: u32 = 3;

pub struct ImageCacheService {
    cache_dir: PathBuf,
    client: reqwest::Client,
    syncing: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageCacheStatus {
    pub cache_dir: String,
    pub cached_files: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageSyncEntry {
    pub grp_id: i32,
    pub url: String,
}

impl ImageCacheService {
    pub fn new(cache_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("IMG-001: Failed to create image cache dir: {}", e))?;

        let client = reqwest::Client::builder()
            .user_agent("mtga-hub/0.1")
            .pool_max_idle_per_host(CONCURRENCY)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("IMG-002: Failed to build HTTP client: {}", e))?;

        Ok(Self {
            cache_dir,
            client,
            syncing: AtomicBool::new(false),
        })
    }

    pub fn status(&self) -> ImageCacheStatus {
        let cached_files = match std::fs::read_dir(&self.cache_dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".jpg") {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        ImageCacheStatus {
            cache_dir: self.cache_dir.to_string_lossy().to_string(),
            cached_files,
        }
    }

    pub async fn sync(
        &self,
        app: &tauri::AppHandle,
        entries: Vec<ImageSyncEntry>,
    ) -> Result<(), String> {
        // Concurrent sync guard — only one sync at a time
        if self
            .syncing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            log::info!("Image cache sync already in progress, skipping");
            return Ok(());
        }

        // Ensure we reset syncing on all exit paths
        let _guard = SyncGuard(&self.syncing);

        // Filter to missing files only
        let missing: Vec<ImageSyncEntry> = entries
            .into_iter()
            .filter(|e| {
                let path = self.cache_dir.join(format!("{}_normal.jpg", e.grp_id));
                !path.exists()
            })
            .collect();

        let total = missing.len();
        if total == 0 {
            log::info!("Image cache: all files present, nothing to download");
            let _ = app.emit(
                events::image_cache::COMPLETE,
                &events::ImageCacheCompletePayload {
                    total_downloaded: 0,
                    total_skipped: 0,
                    total_failed: 0,
                },
            );
            return Ok(());
        }

        log::info!("Image cache: downloading {} missing images", total);

        let semaphore = std::sync::Arc::new(Semaphore::new(CONCURRENCY));
        let completed = std::sync::Arc::new(AtomicUsize::new(0));
        let failed = std::sync::Arc::new(AtomicUsize::new(0));
        let new_files = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));

        let mut handles = Vec::with_capacity(total);

        for entry in missing {
            let sem = semaphore.clone();
            let client = self.client.clone();
            let cache_dir = self.cache_dir.clone();
            let completed = completed.clone();
            let failed = failed.clone();
            let new_files = new_files.clone();
            let app = app.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("Semaphore closed");

                let filename = format!("{}_normal.jpg", entry.grp_id);
                let final_path = cache_dir.join(&filename);
                let tmp_path = cache_dir.join(format!("{}_normal.tmp", entry.grp_id));

                // Retry loop with exponential backoff
                let mut success = false;
                for attempt in 0..MAX_RETRIES {
                    if attempt > 0 {
                        let delay = std::time::Duration::from_secs(1 << attempt);
                        tokio::time::sleep(delay).await;
                    }

                    let result: Result<reqwest::Response, reqwest::Error> =
                        client.get(&entry.url).send().await;
                    match result {
                        Ok(resp) => {
                            if !resp.status().is_success() {
                                log::warn!(
                                    "IMG-011: HTTP {} for grp_id={} (attempt {})",
                                    resp.status(),
                                    entry.grp_id,
                                    attempt + 1
                                );
                                continue;
                            }

                            match resp.bytes().await {
                                Ok(bytes) => {
                                    // Atomic write: tmp → rename
                                    if let Err(e) = tokio::fs::write(&tmp_path, &bytes).await {
                                        log::warn!(
                                            "IMG-013: Write failed for grp_id={}: {}",
                                            entry.grp_id,
                                            e
                                        );
                                        continue;
                                    }
                                    if let Err(e) = tokio::fs::rename(&tmp_path, &final_path).await
                                    {
                                        log::warn!(
                                            "IMG-013: Rename failed for grp_id={}: {}",
                                            entry.grp_id,
                                            e
                                        );
                                        let _ = tokio::fs::remove_file(&tmp_path).await;
                                        continue;
                                    }
                                    success = true;
                                    break;
                                }
                                Err(e) => {
                                    log::warn!(
                                        "IMG-012: Body read failed for grp_id={}: {} (attempt {})",
                                        entry.grp_id,
                                        e,
                                        attempt + 1
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "IMG-010: Request failed for grp_id={}: {} (attempt {})",
                                entry.grp_id,
                                e,
                                attempt + 1
                            );
                        }
                    }
                }

                if success {
                    let mut files = new_files.lock().await;
                    files.push(filename);
                } else {
                    failed.fetch_add(1, Ordering::Relaxed);
                }

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;

                // Emit progress every PROGRESS_INTERVAL downloads
                if done % PROGRESS_INTERVAL == 0 || done == total {
                    let batch: Vec<String> = {
                        let mut files = new_files.lock().await;
                        std::mem::take(&mut *files)
                    };
                    let _ = app.emit(
                        events::image_cache::PROGRESS,
                        &events::ImageCacheProgressPayload {
                            completed: done,
                            total,
                            new_files: batch,
                        },
                    );
                }
            });

            handles.push(handle);
        }

        // Wait for all downloads
        for handle in handles {
            let _ = handle.await;
        }

        // Flush any remaining new_files not yet emitted
        let remaining: Vec<String> = {
            let mut files = new_files.lock().await;
            std::mem::take(&mut *files)
        };
        if !remaining.is_empty() {
            let done = completed.load(Ordering::Relaxed);
            let _ = app.emit(
                events::image_cache::PROGRESS,
                &events::ImageCacheProgressPayload {
                    completed: done,
                    total,
                    new_files: remaining,
                },
            );
        }

        let total_failed = failed.load(Ordering::Relaxed);
        let total_downloaded = total - total_failed;

        log::info!(
            "Image cache sync complete: {} downloaded, {} failed",
            total_downloaded,
            total_failed
        );

        let _ = app.emit(
            events::image_cache::COMPLETE,
            &events::ImageCacheCompletePayload {
                total_downloaded,
                total_skipped: 0,
                total_failed,
            },
        );

        Ok(())
    }
}

/// RAII guard to reset the syncing flag when sync exits (success or error).
struct SyncGuard<'a>(&'a AtomicBool);

impl Drop for SyncGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
