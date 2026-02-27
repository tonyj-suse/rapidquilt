// Licensed under the MIT license. See LICENSE.md

use std::marker::PhantomData;
use std::vec::Vec;
use std::io;
use std::fs::File;
use std::ptr;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::Mutex;

use super::{Arena, Stats, Resource, Mapping};


/// Utility that reads files and keeps them loaded in immovable place in memory
/// for its lifetime. So the returned byte slices can be used as long as the
/// object of this struct is alive.
///
/// This implementation uses mmap, which means that if an external process
/// changes the file, the content of the memory may change or cause crash if
/// the file truncated.
pub struct MmapArena<'a> {
    resources: Mutex<Vec<Resource>>,
    _phantom: PhantomData<&'a [u8]>,
}

// We have `*mut libc::c_void` in there, but we don't use it to mutate anything
// concurently. So no worries...
unsafe impl Sync for MmapArena<'_> {}

impl MmapArena<'_> {
    pub fn new() -> Self {
        Self {
            resources: Mutex::new(Vec::new()),
            _phantom: PhantomData,
        }
    }
}

impl<'a> Arena for MmapArena<'a> {
    /// Load the file and return byte slice of its complete content. The slice
    /// is valid as long as this object is alive. (Same lifetimes.)
    fn load_file(&self, path: &Path) -> Result<&[u8], io::Error> {
        let file = File::open(path)?;
        let size = file.metadata()?.len() as usize;
        let fd = file.as_raw_fd();

        let start = unsafe {
            let start = libc::mmap(ptr::null_mut(),
                size,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                fd,
                0
            );

            if start == libc::MAP_FAILED {
                return Err(io::Error::last_os_error());
            }

            start
        };

        let mapping = Mapping {
            start,
            size,
        };

        let slice = unsafe {
            std::slice::from_raw_parts::<'a>(start as *const u8, size)
        };

        self.resources.lock().unwrap().push(Resource::Mapping(mapping)); // NOTE(unwrap): If the lock is poisoned, some other thread panicked. We may as well.

        Ok(slice)
    }

    fn load_symlink_target(&self, path: &Path) -> Result<&[u8], io::Error> {
        use std::fs;
        use std::mem::transmute;

        let target = fs::read_link(path)?;
        let data = {
            use std::os::unix::ffi::OsStrExt;
            target.as_os_str().as_bytes().to_vec()
        };

        let data = data.into_boxed_slice();

        let slice = unsafe {
            transmute::<&[u8], &'a [u8]>(&data)
        };

        self.resources.lock().unwrap().push(Resource::Data(data));

        Ok(slice)
    }

    /// Get statistics
    fn stats(&self) -> Stats {
        let resources = self.resources.lock().unwrap(); // NOTE(unwrap): If the lock is poisoned, some other thread panicked. We may as well.

        let mut total_size = 0;
        for r in resources.iter() {
            total_size += match r {
                Resource::Mapping(m) => m.size,
                Resource::Data(d) => d.len(),
            };
        }

        Stats {
            loaded_files: resources.len(),
            total_size,
        }
    }
}

impl Drop for MmapArena<'_> {
    fn drop(&mut self) {
        if let Ok(resources) = self.resources.lock() {
            for r in resources.iter() {
                if let Resource::Mapping(m) = r {
                    unsafe {
                        libc::munmap(m.start, m.size);
                    }
                }
            }
        }
    }
}
