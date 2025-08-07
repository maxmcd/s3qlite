use parking_lot::Mutex;
use slatedb::config::{PutOptions, WriteOptions};
use slatedb::object_store::{ObjectStore, memory::InMemory};
use slatedb::{Db, WriteBatch};
use sqlite_plugin::flags;
use sqlite_plugin::vfs;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use tracing::instrument;
use tracing_chrome::ChromeLayerBuilder;
use tracing_subscriber::{Registry, layer::SubscriberExt};
mod handle;

#[derive(Clone)]
struct Capabilities {
    atomic_batch: bool,
    point_in_time_reads: bool,
    sector_size: i32,
}

struct BatchWrite {
    offset: usize,
    data: Vec<u8>,
}

#[derive(Clone)]
struct FileState {
    pending_writes: Arc<Mutex<Vec<BatchWrite>>>,
    batch_open: Arc<AtomicBool>,
}

impl FileState {
    fn new() -> Self {
        Self {
            pending_writes: Arc::new(Mutex::new(Vec::new())),
            batch_open: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone)]
struct GrpcVfs {
    runtime: Arc<tokio::runtime::Runtime>,
    capabilities: Capabilities,
    db: Arc<Db>,
    files: Arc<Mutex<HashMap<String, FileState>>>,
    _guard: Arc<Mutex<tracing_chrome::FlushGuard>>,
}

const PAGE_SIZE: usize = 4096;

impl GrpcVfs {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_time()
            .enable_io()
            .build()
            .unwrap();

        let db = runtime.block_on(async {
            let object_store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
            Db::open("test_db", object_store).await.unwrap()
        });
        let guard = setup_tracing();

        Self {
            db: Arc::new(db),
            runtime: Arc::new(runtime),
            files: Arc::new(Mutex::new(HashMap::new())),
            capabilities: Capabilities {
                atomic_batch: true,
                point_in_time_reads: false,
                sector_size: 4096,
            },
            _guard: Arc::new(Mutex::new(guard)),
        }
    }
}

impl vfs::Vfs for GrpcVfs {
    type Handle = handle::GrpcVfsHandle;

    fn register_logger(&self, logger: sqlite_plugin::logger::SqliteLogger) {
        struct LogCompat {
            logger: Mutex<sqlite_plugin::logger::SqliteLogger>,
        }

        impl log::Log for LogCompat {
            fn enabled(&self, _metadata: &log::Metadata) -> bool {
                true
            }

            fn log(&self, record: &log::Record) {
                let level = match record.level() {
                    log::Level::Error => sqlite_plugin::logger::SqliteLogLevel::Error,
                    log::Level::Warn => sqlite_plugin::logger::SqliteLogLevel::Warn,
                    _ => sqlite_plugin::logger::SqliteLogLevel::Notice,
                };
                if !record.target().contains("s3qlite") {
                    // Filter out logs from other modules.
                    return;
                }
                let msg = format!("{}", record.args());
                println!("{msg}");
                self.logger.lock().log(level, msg.as_bytes());
            }

            fn flush(&self) {}
        }

        let log = LogCompat {
            logger: Mutex::new(logger),
        };
        if let Err(e) = log::set_boxed_logger(Box::new(log)) {
            // Logger already set, ignore the error
            eprintln!("Logger already initialized: {e}");
        }
    }

    #[instrument(level = "info", skip(self))]
    fn open(&self, path: Option<&str>, opts: flags::OpenOpts) -> vfs::VfsResult<Self::Handle> {
        let path = path.unwrap_or("");
        log::debug!("open: path={path}, opts={opts:?}");
        let mode = opts.mode();

        if mode.is_readonly() && !self.capabilities.point_in_time_reads {
            log::error!("read-only mode is not supported for this server");
            return Err(sqlite_plugin::vars::SQLITE_CANTOPEN);
        }

        if !path.is_empty() {
            self.runtime.block_on(async {
                let db = self.db.clone();
                db.put_with_options(
                    &path,
                    &[],
                    &PutOptions::default(),
                    &WriteOptions {
                        await_durable: false,
                    },
                )
                .await
                .map_err(|e| {
                    log::error!("error putting page key: {e}");
                    sqlite_plugin::vars::SQLITE_IOERR_DELETE
                })
            })?;
        }

        let handle = handle::GrpcVfsHandle::new(path.to_string(), mode.is_readonly());
        Ok(handle)
    }

    #[instrument(level = "info", skip(self))]
    fn delete(&self, path: &str) -> vfs::VfsResult<()> {
        log::debug!("delete: path={path}");

        self.runtime.block_on(async {
            let db = self.db.clone();

            // Delete all pages for this file
            let mut page_offset = 0;
            loop {
                let page_key = format!("{path}:page:{page_offset}");
                let exists = db.get(&page_key).await.map_err(|e| {
                    log::error!("error getting page key: {e}");
                    sqlite_plugin::vars::SQLITE_IOERR_DELETE
                })?;

                if exists.is_some() {
                    db.delete_with_options(
                        &page_key,
                        &WriteOptions {
                            await_durable: false,
                        },
                    )
                    .await
                    .map_err(|e| {
                        log::error!("error deleting page key: {e}");
                        sqlite_plugin::vars::SQLITE_IOERR_DELETE
                    })?;
                    page_offset += PAGE_SIZE;
                } else {
                    break;
                }
            }
            db.delete_with_options(
                &path,
                &WriteOptions {
                    await_durable: false,
                },
            )
            .await
            .map_err(|e| {
                log::error!("error deleting file: {e}");
                sqlite_plugin::vars::SQLITE_IOERR_DELETE
            })?;
            Ok::<(), i32>(())
        })?;

        Ok(())
    }

    #[instrument(level = "info", skip(self))]
    fn access(&self, path: &str, flags: flags::AccessFlags) -> vfs::VfsResult<bool> {
        let exists = self.runtime.block_on(async {
            let db = self.db.clone();
            db.get(&path).await.map_err(|e| {
                log::error!("error getting page key: {e}");
                sqlite_plugin::vars::SQLITE_IOERR_ACCESS
            })
        })?;
        let exists = exists.is_some();
        log::debug!("access: path={path}, flags={flags:?}, exists={exists}");
        Ok(exists)
    }

    #[instrument(level = "info", skip(self))]
    fn file_size(&self, handle: &mut Self::Handle) -> vfs::VfsResult<usize> {
        let max_size = self.runtime.block_on(async {
            let db = self.db.clone();

            // Find the highest page offset for this file to calculate total size
            // This is a simplified approach - in a real implementation you might want to
            // track file metadata separately for better performance
            let mut max_size = 0usize;

            // Check pages starting from 0 until we find no more
            let mut page_offset = 0;
            loop {
                let page_key = format!("{}:page:{}", handle.path, page_offset);
                let page_data = db.get(&page_key).await.map_err(|e| {
                    log::error!("error getting page key: {e}");
                    sqlite_plugin::vars::SQLITE_IOERR_FSTAT
                })?;

                if let Some(page) = page_data {
                    max_size = page_offset + page.len();
                    page_offset += PAGE_SIZE;
                } else {
                    break;
                }
            }

            Ok::<usize, i32>(max_size)
        })?;

        Ok(max_size)
    }

    #[instrument(level = "info", skip(self))]
    fn truncate(&self, handle: &mut Self::Handle, size: usize) -> vfs::VfsResult<()> {
        if size == 0 {
            self.delete(handle.path.as_str())?;
            return Ok(());
        }

        self.runtime.block_on(async {
            let db = self.db.clone();
            // Calculate which page contains the truncation point
            let truncate_page_offset = (size / PAGE_SIZE) * PAGE_SIZE;
            let truncate_offset_in_page = size % PAGE_SIZE;

            // Truncate the page that contains the truncation point
            let page_key = format!("{}:page:{}", handle.path, truncate_page_offset);
            let page_data = db.get(&page_key).await.map_err(|e| {
                log::error!("error getting page during truncate: {e}");
                sqlite_plugin::vars::SQLITE_IOERR_TRUNCATE
            })?;

            if let Some(page) = page_data {
                let mut page_vec = page.clone();
                if truncate_offset_in_page < page_vec.len() {
                    page_vec.truncate(truncate_offset_in_page);
                    db.put_with_options(
                        &page_key,
                        page_vec,
                        &PutOptions::default(),
                        &WriteOptions {
                            await_durable: false,
                        },
                    )
                    .await
                    .map_err(|e| {
                        log::error!("error putting truncated page: {e}");
                        sqlite_plugin::vars::SQLITE_IOERR_TRUNCATE
                    })?;
                }
            }

            // Delete all pages beyond the truncation point
            let mut page_offset = truncate_page_offset + PAGE_SIZE;
            loop {
                let page_key = format!("{}:page:{}", handle.path, page_offset);
                let exists = db.get(&page_key).await.map_err(|e| {
                    log::error!("error checking page existence during truncate: {e}");
                    sqlite_plugin::vars::SQLITE_IOERR_TRUNCATE
                })?;

                if exists.is_some() {
                    db.delete_with_options(
                        &page_key,
                        &WriteOptions {
                            await_durable: false,
                        },
                    )
                    .await
                    .map_err(|e| {
                        log::error!("error deleting page during truncate: {e}");
                        sqlite_plugin::vars::SQLITE_IOERR_TRUNCATE
                    })?;
                    page_offset += PAGE_SIZE;
                } else {
                    break;
                }
            }

            Ok::<(), i32>(())
        })?;

        Ok(())
    }

    #[instrument(level = "info", skip(self, data))]
    fn write(
        &self,
        handle: &mut Self::Handle,
        offset: usize,
        data: &[u8],
    ) -> vfs::VfsResult<usize> {
        // Get or create file state
        let file_state = {
            let mut files = self.files.lock();
            files
                .entry(handle.path.clone())
                .or_insert_with(FileState::new)
                .clone()
        };
        let is_batch_write = file_state.batch_open.load(Ordering::Acquire);
        log::debug!(
            "write: path={}, offset={offset}, is_batch_write={is_batch_write}",
            handle.path
        );

        // Check if we're in batch mode for this file
        if is_batch_write {
            let mut pending_writes = file_state.pending_writes.lock();
            pending_writes.push(BatchWrite {
                offset,
                data: data.to_vec(),
            });
            return Ok(data.len());
        }

        // Write over the server
        self.runtime.block_on(async {
            let db = self.db.clone();
            let page_offset = (offset / PAGE_SIZE) * PAGE_SIZE;
            let page_key = format!("{}:page:{}", handle.path, page_offset);

            // Get existing page data
            let existing_page = db.get(&page_key).await.map_err(|e| {
                log::error!("error getting page during write: {e}");
                sqlite_plugin::vars::SQLITE_IOERR_WRITE
            })?;

            let mut page_data = if let Some(existing) = existing_page {
                existing.to_vec()
            } else {
                Vec::new()
            };

            let offset_in_page = offset % PAGE_SIZE;

            // Resize page if needed
            if offset_in_page + data.len() > page_data.len() {
                page_data.resize(offset_in_page + data.len(), 0);
            }

            println!(
                "write data at page {} offset {} length {}",
                page_offset,
                offset_in_page,
                data.len()
            );
            page_data[offset_in_page..offset_in_page + data.len()].copy_from_slice(data);

            db.put_with_options(
                &page_key,
                page_data,
                &PutOptions::default(),
                &WriteOptions {
                    await_durable: false,
                },
            )
            .await
            .map_err(|e| {
                log::error!("error putting page during write: {e}");
                sqlite_plugin::vars::SQLITE_IOERR_WRITE
            })
        })?;
        Ok(data.len())
    }

    #[instrument(level = "info", skip(self, data))]
    fn read(
        &self,
        handle: &mut Self::Handle,
        offset: usize,
        data: &mut [u8],
    ) -> vfs::VfsResult<usize> {
        // Read from the server
        let result = self.runtime.block_on(async {
            let db = self.db.clone();
            // Calculate the page key using integer division
            let page_offset = (offset / PAGE_SIZE) * PAGE_SIZE;
            let page_key = format!("{}:page:{}", handle.path, page_offset);

            let page_data = db.get(&page_key).await.map_err(|e| {
                log::error!("error getting page during read: {e}");
                sqlite_plugin::vars::SQLITE_IOERR_READ
            })?;

            if page_data.is_none() {
                println!("read page not found, returning empty data");
                return Ok::<Vec<u8>, i32>(vec![]);
            }

            let page = page_data.unwrap();
            let offset_in_page = offset % PAGE_SIZE;

            // Check if offset is beyond page size
            if offset_in_page >= page.len() {
                println!("read offset is beyond page size");
                return Ok(vec![]);
            }

            // Read as much data as available from this page, up to the requested length
            let end_offset_in_page = std::cmp::min(offset_in_page + data.len(), page.len());
            let data = page[offset_in_page..end_offset_in_page].to_vec();

            println!("read data length: {} from page {}", data.len(), page_offset);

            Ok(data)
        })?;

        let len = data.len().min(result.len());
        data[..len].copy_from_slice(&result[..len]);
        Ok(len)
    }

    #[instrument(level = "info", skip(self))]
    fn close(&self, handle: Self::Handle) -> vfs::VfsResult<()> {
        self.files.lock().remove(&handle.path);

        // Flush traces on every close to ensure data is written
        let guard = self._guard.lock();
        guard.flush();

        Ok(())
    }

    fn device_characteristics(&self) -> i32 {
        log::debug!("device_characteristics");
        let mut characteristics: i32 = vfs::DEFAULT_DEVICE_CHARACTERISTICS;
        if self.capabilities.atomic_batch {
            characteristics |= sqlite_plugin::vars::SQLITE_IOCAP_BATCH_ATOMIC;
        }
        // TODO: Do we bother with SQLITE_IOCAP_IMMUTABLE if we're opened in read only mode?
        characteristics
    }

    fn pragma(
        &self,
        handle: &mut Self::Handle,
        pragma: vfs::Pragma<'_>,
    ) -> Result<Option<String>, vfs::PragmaErr> {
        log::debug!("pragma: file2={:?}, pragma={:?}", handle.path, pragma);
        if pragma.name == "is_memory_server" {
            return Ok(Some("maybe?".to_string()));
        }
        Ok(None)
    }

    fn file_control(
        &self,
        handle: &mut Self::Handle,
        op: c_int,
        _p_arg: *mut c_void,
    ) -> vfs::VfsResult<()> {
        let op_name = match op {
            sqlite_plugin::vars::SQLITE_FCNTL_BEGIN_ATOMIC_WRITE => "begin_atomic_write",
            sqlite_plugin::vars::SQLITE_FCNTL_COMMIT_ATOMIC_WRITE => "commit_atomic_write",
            sqlite_plugin::vars::SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE => "rollback_atomic_write",
            _ => "",
        };
        let op_name = if op_name.is_empty() {
            format!("{op:?}")
        } else {
            op_name.to_string()
        };
        log::debug!("file_control: file={:?}, op={op_name}", handle.path);
        match op {
            sqlite_plugin::vars::SQLITE_FCNTL_BEGIN_ATOMIC_WRITE => {
                let file_state = {
                    let mut files = self.files.lock();
                    files
                        .entry(handle.path.clone())
                        .or_insert_with(FileState::new)
                        .clone()
                };
                // Open the write batch
                file_state.batch_open.store(true, Ordering::Release);
                Ok(())
            }
            sqlite_plugin::vars::SQLITE_FCNTL_COMMIT_ATOMIC_WRITE => {
                let file_state = {
                    let mut files = self.files.lock();
                    files
                        .entry(handle.path.clone())
                        .or_insert_with(FileState::new)
                        .clone()
                };

                // Close the write batch
                file_state.batch_open.store(false, Ordering::Release);

                // Send the batch over the server
                self.runtime.block_on(async {
                    let batch = {
                        let mut pending = file_state.pending_writes.lock();
                        std::mem::take(&mut *pending)
                    };
                    if batch.is_empty() {
                        log::debug!("write batch is empty, nothing to commit");
                        return Ok(());
                    }
                    let mut page_writes: HashMap<usize, Vec<_>> = HashMap::new();
                    for write in batch.iter() {
                        let offset = write.offset;
                        let page_offset = (offset / PAGE_SIZE) * PAGE_SIZE;

                        page_writes
                            .entry(page_offset)
                            .or_default()
                            .push((offset, write));
                    }
                    let db = self.db.clone();

                    // Prepare WriteBatch for atomic operation
                    let mut batch = WriteBatch::new();

                    // Apply writes to each affected page
                    for (page_offset, writes) in page_writes {
                        let page_key = format!("{}:page:{}", handle.path, page_offset);

                        // Get existing page data
                        let existing_page = db.get(&page_key).await.map_err(|e| {
                            log::error!("error getting page during atomic write: {e}");
                            sqlite_plugin::vars::SQLITE_IOERR_WRITE
                        })?;

                        let mut page_data = if let Some(existing) = existing_page {
                            existing.to_vec()
                        } else {
                            Vec::new()
                        };

                        // Apply all writes for this page
                        for (offset, write) in writes {
                            let offset_in_page = offset % PAGE_SIZE;

                            log::debug!(
                                "atomic_write_batch write page={} offset_in_page={} length={}",
                                page_offset,
                                offset_in_page,
                                write.data.len(),
                            );

                            if offset_in_page + write.data.len() > page_data.len() {
                                page_data.resize(offset_in_page + write.data.len(), 0);
                            }
                            page_data[offset_in_page..offset_in_page + write.data.len()]
                                .copy_from_slice(&write.data);
                        }

                        // Add the page update to the batch
                        batch.put(&page_key, page_data);
                    }

                    // Execute all page updates atomically
                    db.write_with_options(
                        batch,
                        &WriteOptions {
                            await_durable: false,
                        },
                    )
                    .await
                    .map_err(|e| {
                        log::error!("error writing batch: {e}");
                        sqlite_plugin::vars::SQLITE_IOERR_WRITE
                    })
                })?;

                Ok(())
            }
            sqlite_plugin::vars::SQLITE_FCNTL_ROLLBACK_ATOMIC_WRITE => {
                let file_state = {
                    let mut files = self.files.lock();
                    files
                        .entry(handle.path.clone())
                        .or_insert_with(FileState::new)
                        .clone()
                };
                // Close the write batch
                file_state.batch_open.store(false, Ordering::Release);
                // Clear the batch
                file_state.pending_writes.lock().clear();
                Ok(())
            }
            _ => Err(sqlite_plugin::vars::SQLITE_NOTFOUND),
        }
    }

    fn sector_size(&self) -> i32 {
        log::debug!("sector_size");
        self.capabilities.sector_size
    }

    fn unlock(&self, handle: &mut Self::Handle, level: flags::LockLevel) -> vfs::VfsResult<()> {
        log::debug!("unlock: path={} level={level:?}", handle.path);
        Ok(())
    }
    fn lock(&self, handle: &mut Self::Handle, level: flags::LockLevel) -> vfs::VfsResult<()> {
        log::debug!("lock: path={} level={level:?}", handle.path);
        Ok(())
    }
    #[instrument(level = "info", skip(self))]
    fn sync(&self, handle: &mut Self::Handle) -> vfs::VfsResult<()> {
        log::debug!("sync: path={}", handle.path);
        self.runtime.block_on(async {
            let db = self.db.clone();
            db.flush().await.map_err(|e| {
                log::error!("error flushing database: {e}");
                sqlite_plugin::vars::SQLITE_IOERR_FSYNC
            })
        })?;
        Ok(())
    }
}

const VFS_NAME: &CStr = c"grpsqlite";

static GRPC_VFS_INSTANCE: OnceLock<Arc<GrpcVfs>> = OnceLock::new();

fn get_grpc_vfs() -> Arc<GrpcVfs> {
    GRPC_VFS_INSTANCE
        .get_or_init(|| Arc::new(GrpcVfs::new()))
        .clone()
}

fn setup_tracing() -> tracing_chrome::FlushGuard {
    use std::fs::File;
    use std::io::BufWriter;

    let (chrome_layer, guard) = ChromeLayerBuilder::new()
        .writer(BufWriter::new(
            File::create("s3qlite_trace.cpuprofile").unwrap(),
        ))
        .build();

    // Don't call init() which would take over global logging
    // Instead, just set up tracing without interfering with existing log setup
    let subscriber = Registry::default().with(chrome_layer);

    // Only set the global default if there isn't one already
    let _ = tracing::subscriber::set_global_default(subscriber);

    guard
}

impl Drop for GrpcVfs {
    fn drop(&mut self) {
        // Flush traces when VFS is dropped
        let guard = self._guard.lock();
        guard.flush();
        println!("Flushed traces");
    }
}

/// This function initializes the VFS statically.
/// Called automatically when the library is loaded.
///
/// # Safety
/// This function is safe to call from C as it only registers a VFS implementation
/// with SQLite and doesn't access any raw pointers or perform unsafe operations.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn initialize_grpsqlite() -> i32 {
    let vfs = get_grpc_vfs();

    if let Err(err) = vfs::register_static(
        VFS_NAME.to_owned(),
        (*vfs).clone(),
        vfs::RegisterOpts { make_default: true },
    ) {
        eprintln!("Failed to initialize grpsqlite: {err}");
        return err;
    }

    // set the log level to trace
    log::set_max_level(log::LevelFilter::Trace);
    sqlite_plugin::vars::SQLITE_OK
}

/// This function is called by `SQLite` when the extension is loaded. It registers
/// the memvfs VFS with `SQLite`.
///
/// # Safety
/// This function should only be called by sqlite's extension loading mechanism.
/// The provided pointers must be valid SQLite API structures.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sqlite3_grpsqlite_init(
    _db: *mut c_void,
    _pz_err_msg: *mut *mut c_char,
    p_api: *mut sqlite_plugin::sqlite3_api_routines,
) -> std::os::raw::c_int {
    let vfs = get_grpc_vfs();
    if let Err(err) = unsafe {
        vfs::register_dynamic(
            p_api,
            VFS_NAME.to_owned(),
            (*vfs).clone(),
            vfs::RegisterOpts { make_default: true },
        )
    } {
        return err;
    }

    // set the log level to trace
    log::set_max_level(log::LevelFilter::Trace);

    sqlite_plugin::vars::SQLITE_OK_LOAD_PERMANENTLY
}
