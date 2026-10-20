//! Audio that plays while it downloads. A podcast episode runs to a hundred
//! megabytes, too much to wait for, so it goes to a cache file and the
//! decoder reads that file as it fills, waiting for bytes that haven't
//! arrived yet. A download cut short picks up where it stopped next time.

use anyhow::{Context, Result, anyhow, bail};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// No episode is bigger than this; a feed whose audio is (or never ends)
/// is cut off rather than filling the disk.
const MAX_BYTES: u64 = 1 << 30;
/// Nor takes longer than this to download, however it trickles in.
const MAX_TIME: std::time::Duration = std::time::Duration::from_secs(2 * 60 * 60);

#[derive(Default)]
struct Progress {
    written: u64,
    total: Option<u64>,
    done: bool,
    failed: Option<String>,
}

struct Inner {
    progress: Mutex<Progress>,
    arrived: Condvar,
    // Nobody wants it any more: the download stops.
    cancelled: AtomicBool,
}

impl Inner {
    fn update(&self, change: impl FnOnce(&mut Progress)) {
        change(&mut self.progress.lock().unwrap());
        self.arrived.notify_all();
    }
}

/// One episode's download. Dropping the last handle (and every reader made
/// from it) stops the download.
pub struct Download {
    inner: Arc<Inner>,
    path: PathBuf,
}

impl Drop for Download {
    fn drop(&mut self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.arrived.notify_all();
    }
}

/// The partial file beside the finished one.
fn part_of(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

impl Download {
    /// Start (or pick up, or reuse) the download of `url` into `path`.
    pub fn start(http: reqwest::blocking::Client, url: &str, path: PathBuf) -> Result<Arc<Self>> {
        let inner = Arc::new(Inner { progress: Mutex::new(Progress::default()), arrived: Condvar::new(), cancelled: AtomicBool::new(false) });
        if let Ok(meta) = std::fs::metadata(&path) {
            // Already here in full; touched so the cache keeps it longest.
            let _ = File::options().write(true).open(&path).and_then(|f| f.set_modified(std::time::SystemTime::now()));
            inner.update(|p| {
                p.written = meta.len();
                p.total = Some(meta.len());
                p.done = true;
            });
            return Ok(Arc::new(Self { inner, path }));
        }
        // Its folder and the cache's own (~/.cache/omadora) both private.
        if let Some(dir) = path.parent() {
            if let Some(cache) = dir.parent() {
                crate::files::private_dir(cache)?;
            }
            crate::files::private_dir(dir)?;
        }
        let part = part_of(&path);
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let mut request = http.get(url);
        if have > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let response = request.send().map_err(reqwest::Error::without_url)?.error_for_status().map_err(reqwest::Error::without_url)?;
        // A server that ignores the range sends it all again: start over.
        let resumed = have > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let total = if resumed {
            response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|n| n.parse().ok())
        } else {
            response.content_length()
        };
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            File::options().create(true).write(true).append(resumed).truncate(!resumed).mode(0o600).open(&part).context("opening the episode's cache file")?
        };
        if total.is_some_and(|t| t > MAX_BYTES) {
            bail!("this episode is too large");
        }
        let start = if resumed { have } else { 0 };
        inner.update(|p| {
            p.written = start;
            p.total = total;
        });
        let download = Arc::new(Self { inner: inner.clone(), path: path.clone() });
        std::thread::spawn(move || {
            let mut response = response;
            let mut buf = vec![0u8; 64 * 1024];
            let began = std::time::Instant::now();
            let result = (|| -> Result<()> {
                loop {
                    if began.elapsed() > MAX_TIME {
                        return Err(anyhow!("this episode took too long to download"));
                    }
                    if inner.cancelled.load(Ordering::SeqCst) {
                        return Err(anyhow!("stopped"));
                    }
                    let n = response.read(&mut buf)?;
                    if n == 0 {
                        return Ok(());
                    }
                    if inner.progress.lock().unwrap().written + n as u64 > MAX_BYTES {
                        return Err(anyhow!("this episode is too large"));
                    }
                    file.write_all(&buf[..n])?;
                    inner.update(|p| p.written += n as u64);
                }
            })();
            let complete = {
                let p = inner.progress.lock().unwrap();
                result.is_ok() && p.total.is_none_or(|t| p.written >= t)
            };
            if complete {
                let _ = file.flush();
                let _ = std::fs::rename(&part, &path);
                inner.update(|p| {
                    p.total = Some(p.written);
                    p.done = true;
                });
            } else {
                let error = result.err().map(|e| format!("{e:#}")).unwrap_or_else(|| "the download ended early".into());
                inner.update(|p| p.failed = Some(error));
            }
        });
        Ok(download)
    }

    /// Its size, once the server has said.
    pub fn total(&self) -> Option<u64> {
        self.inner.progress.lock().unwrap().total
    }

    /// Wait until the first `bytes` are here (or all of it, or it failed),
    /// for at most `limit`. True when they are.
    pub fn wait_for(&self, bytes: u64, limit: Duration) -> bool {
        let progress = self.inner.progress.lock().unwrap();
        let (progress, _) = self
            .inner
            .arrived
            .wait_timeout_while(progress, limit, |p| p.written < bytes && !p.done && p.failed.is_none())
            .unwrap();
        progress.written >= bytes || progress.done
    }

    pub fn failure(&self) -> Option<String> {
        self.inner.progress.lock().unwrap().failed.clone()
    }

    /// A reader from the start of the file, for the decoder.
    pub fn reader(self: &Arc<Self>) -> Result<Reader> {
        // The partial file is renamed when complete; whichever is there now.
        let file = File::open(part_of(&self.path)).or_else(|_| File::open(&self.path)).context("opening the episode's cache file")?;
        Ok(Reader { file, pos: 0, download: self.clone() })
    }
}

/// Reads the cache file as it fills, waiting at its edge.
pub struct Reader {
    file: File,
    pos: u64,
    download: Arc<Download>,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let inner = &self.download.inner;
        let mut progress = inner.progress.lock().unwrap();
        loop {
            if self.pos < progress.written {
                let available = (progress.written - self.pos).min(buf.len() as u64) as usize;
                drop(progress);
                self.file.seek(SeekFrom::Start(self.pos))?;
                let n = self.file.read(&mut buf[..available])?;
                self.pos += n as u64;
                return Ok(n);
            }
            if progress.done {
                return Ok(0);
            }
            if let Some(error) = &progress.failed {
                return Err(std::io::Error::other(error.clone()));
            }
            if inner.cancelled.load(Ordering::SeqCst) {
                return Err(std::io::Error::other("stopped"));
            }
            progress = inner.arrived.wait_timeout(progress, Duration::from_secs(1)).unwrap().0;
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let next = match to {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => match self.download.total() {
                Some(total) => total as i64 + d,
                None => return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "length not known yet")),
            },
        };
        if next < 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "before the start"));
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

/// Keep the cache under `max` bytes, oldest first, sparing `keep`.
pub fn prune(dir: &Path, max: u64, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file().then(|| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    for (_, len, path) in files {
        if total <= max {
            break;
        }
        if path == keep || path == part_of(keep) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_waits_for_bytes_then_reads_them() {
        let dir = std::env::temp_dir().join(format!("omadora-stream-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("episode.mp3");
        std::fs::write(part_of(&path), b"").unwrap();
        let inner = Arc::new(Inner { progress: Mutex::new(Progress { total: Some(6), ..Default::default() }), arrived: Condvar::new(), cancelled: AtomicBool::new(false) });
        let download = Arc::new(Download { inner: inner.clone(), path: path.clone() });
        let mut reader = download.reader().unwrap();
        let writer = {
            let part = part_of(&path);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                std::fs::write(&part, b"hello!").unwrap();
                inner.update(|p| {
                    p.written = 6;
                    p.done = true;
                });
            })
        };
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        writer.join().unwrap();
        assert_eq!(out, b"hello!");
        assert_eq!(reader.seek(SeekFrom::End(-1)).unwrap(), 5);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
