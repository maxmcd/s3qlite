#[derive(Clone, Debug)]
pub struct GrpcVfsHandle {
    pub path: String,
    readonly: bool,
}

impl GrpcVfsHandle {
    pub fn new(path: String, readonly: bool) -> Self {
        Self { path, readonly }
    }
}

impl sqlite_plugin::vfs::VfsHandle for GrpcVfsHandle {
    fn readonly(&self) -> bool {
        self.readonly
    }

    fn in_memory(&self) -> bool {
        // TODO does this matter?
        false
    }
}
