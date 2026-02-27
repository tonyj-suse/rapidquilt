use crate::cmd;

use std::fs;

use std::ffi::OsStr;
use std::path::Path;
use std::io::{Read, ErrorKind};
use anyhow::{anyhow, Context, Result};

#[cfg(test)]
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    for entry in fs::read_dir(from).context(format!("Copying {:?}", from))? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = to.join(entry.file_name());
        if src_path.is_file() {
            fs::copy(&src_path, &dest_path)
                .context(format!("Copying {:?} under {:?}", src_path, to))?;
        } else if src_path.is_dir() {
            fs::create_dir(&dest_path)
                .context(format!("Creating directory {:?}", dest_path))?;
            copy_tree(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn compare_tree(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src).context(format!("Reading {:?}", src))? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dst.join(entry.file_name());

        let src_meta = fs::symlink_metadata(&src_path)
            .context(format!("Querying {:?} metadata", src_path))?;
        let dest_meta = fs::symlink_metadata(&dest_path)
            .context(format!("Querying {:?} metadata", dest_path))?;

        if src_meta.permissions() != dest_meta.permissions() {
            return Err(anyhow!("Permission mismatch at {:?}: expected {:?}, actual {:?}",
                               dest_path, src_meta.permissions(), dest_meta.permissions()));
        }

        if src_meta.file_type().is_symlink() {
            if !dest_meta.file_type().is_symlink() {
                return Err(anyhow!("Expected symlink at {:?}, but found regular file", dest_path));
            }
            let src_target = fs::read_link(&src_path)?;
            let dest_target = fs::read_link(&dest_path)?;
            if src_target != dest_target {
                return Err(anyhow!("Symlink target mismatch at {:?}: expected {:?}, actual {:?}",
                                   dest_path, src_target, dest_target));
            }
        } else if src_meta.is_file() {
            let mut src_file = std::fs::File::open(&src_path)
                .context(format!("Opening {:?}", src_path))?;
            let mut dest_file = std::fs::File::open(&dest_path)
                .context(format!("Opening {:?}", dest_path))?;

            let mut expected = Vec::new();
            src_file.read_to_end(&mut expected)
                .context(format!("Reading {:?}", src_path))?;
            let mut actual = Vec::new();
            dest_file.read_to_end(&mut actual)
                .context(format!("Reading {:?}", dest_path))?;
            if actual != expected {
                return Err(anyhow!("Content mismatch in {:?}:\n<<< EXPECTED\n{}=== ACTUAL\n{}\n>>>",
                                   dest_path,
                                   String::from_utf8_lossy(&expected),
                                   String::from_utf8_lossy(&actual)));
            }
        } else if src_meta.is_dir() {
            compare_tree(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
fn check_extra_files(src: &Path, dst: &Path) -> Result<()> {
    let mut errors = Vec::<String>::new();
    for entry in fs::read_dir(dst).context(format!("Reading {:?}", dst))? {
        let entry = entry?;
        let dst_path = entry.path();
        let src_path = src.join(entry.file_name());
        if !src_path.exists() {
            errors.push(format!("Unexpected file {:?}", dst_path));
        } else if dst_path.is_dir() {
            check_extra_files(&src_path, &dst_path)?;
        }
    }
    match errors.len() {
        0 => Ok(()),
        _ => Err(anyhow!(errors.join("\n"))),
    }
}

#[cfg(test)]
fn push_all(path: &Path, num_threads: usize, expect: bool) -> Result<()> {
    eprintln!("Push all patches in {}", path.display());

    let work_dir = tempfile::tempdir()?;
    let work_path = work_dir.path();
    copy_tree(&path.join("input"), &work_path)?;

    let num_threads = num_threads.to_string();
    let args = [
        OsStr::new("push"),
        OsStr::new("--quiet"),
        OsStr::new("--threads"), OsStr::new(&num_threads),
        OsStr::new("--all"),
        OsStr::new("--directory"), work_path.as_os_str(),
        OsStr::new("--backup"), OsStr::new("always"),
    ];
    let result = cmd::run(&args);

    match result {
        Ok(status) if status == expect => {
            compare_tree(&path.join("expect"), &work_path)?;
            check_extra_files(&path.join("expect"), &work_path)
        },
        Ok(_) => Err(anyhow!(match expect {
            true => "Push failed unexpectedly",
            false => "Push was expected to fail but it did not",
        })),
        Err(err) => Err(err)
    }
}

#[cfg(test)]
fn check_series(path: &str, num_threads: usize, expect: bool) -> Result<()> {
    let dir = fs::read_dir(path);
    match dir {
        Ok(dir) => {
            for entry in dir {
                let entry = entry?;
                if let Err(err) = push_all(&entry.path(), num_threads, expect) {
                    for fail in err.chain() {
                        eprintln!("{}", fail);
                    }
                    panic!("Push all failed for {:?}", entry.file_name());
                }
            }
            Ok(())
        },
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
#[test]
fn ok_series_sequential() -> Result<()> {
    check_series("testdata/quilt/ok", 1, true)
}

#[cfg(test)]
#[test]
fn fail_series_sequential() -> Result<()> {
    check_series("testdata/quilt/fail", 1, false)
}

#[cfg(test)]
const NUM_THREADS: usize = 2;

#[cfg(test)]
#[test]
fn ok_series_parallel() -> Result<()> {
    check_series("testdata/quilt/ok", NUM_THREADS, true)
}

#[cfg(test)]
#[test]
fn fail_series_parallel() -> Result<()> {
    check_series("testdata/quilt/fail", NUM_THREADS, false)
}
