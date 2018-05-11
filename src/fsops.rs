extern crate colored;
extern crate filetime;

use std;
use std::io;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Write;
use std::fs;
use std::os::unix;
use std::fs::File;
use std::path::Path;

use self::colored::Colorize;
use self::filetime::FileTime;

use entry::Entry;

const BUFFER_SIZE: usize = 100 * 1024;

pub fn to_io_error(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::Other, message)
}

fn more_recent_than(src: &Entry, dest: &Entry) -> io::Result<bool> {
    if !dest.exists() {
        return Ok(true);
    }

    let src_meta = &src.metadata();
    let dest_meta = &dest.metadata();

    let src_meta = &src_meta.expect("src_meta was None");
    let dest_meta = &dest_meta.expect("dest_meta was None");

    let src_mtime = FileTime::from_last_modification_time(&src_meta);
    let dest_mtime = FileTime::from_last_modification_time(&dest_meta);

    let src_precise = src_mtime.seconds() * 1000 * 1000 * 1000 + src_mtime.nanoseconds() as u64;
    let dest_precise = dest_mtime.seconds() * 1000 * 1000 * 1000 + dest_mtime.nanoseconds() as u64;

    Ok(src_precise > dest_precise)
}

fn is_link(path: &Path) -> io::Result<bool> {
    let metadata = std::fs::symlink_metadata(path)?;
    Ok(metadata.file_type().is_symlink())
}

fn copy_perms(src: &Entry, dest: &Entry) -> io::Result<()> {
    let src_meta = &src.metadata();
    let src_meta = &src_meta.expect("src_meta was None");
    let permissions = src_meta.permissions();
    let dest_file = File::create(dest.path())?;
    dest_file.set_permissions(permissions)?;
    Ok(())
}

fn copy_link(src: &Entry, dest: &Entry) -> io::Result<(bool)> {
    let src_target =  std::fs::read_link(src.path())?;
    let is_link_outcome = is_link(&dest.path());
    match is_link_outcome {
        Ok(true) => {
            let dest_target = std::fs::read_link(dest.path())?;
            if dest_target != src_target {
                println!("{} {}", "--".red(), src.description().bold());
                fs::remove_file(dest.path())?
            } else {
               return Ok(false)
            }

        }
        Ok(false) => {
            // Never safe to delete
            return Err(
                to_io_error(
                    String::from(
                        format!("Refusing to replace existing path {:?} by symlink", dest.path()))));
        }
        Err(_) => {
            // OK, dest does not exist
        }
    }
    println!("{} {} -> {}", "++".blue(), src.description().bold(), src_target.to_string_lossy());
    unix::fs::symlink(src_target, &dest.path())?;
    Ok(true)
}

pub fn copy_entry(src: &Entry, dest: &Entry) -> io::Result<(bool)> {
    let src_path = src.path();
    let src_file = File::open(src_path)?;
    let src_meta = src.metadata().expect("src_meta should not be None");
    let src_size = src_meta.len();
    let mut done = 0;
    let mut buf_reader = BufReader::new(src_file);
    let dest_path = dest.path();
    let dest_file = File::create(dest_path)?;
    let mut buf_writer = BufWriter::new(dest_file);
    let mut buffer = vec![0; BUFFER_SIZE];
    println!("{} {}", "++".green(), src.description().bold());
    loop {
        let num_read = buf_reader.read(&mut buffer)?;
        if num_read == 0 {
            break;
        }
        done += num_read;
        let percent = ((done * 100) as u64) / src_size;
        print!("{number:>width$}%\r", number=percent, width=3);
        let _ = io::stdout().flush();
        buf_writer.write(&buffer[0..num_read])?;
    }
    // This is allowed to fail, for instance when
    // copying from an ext4 to a fat32 partition
    let copy_outcome = copy_perms(&src, &dest);
    if let Err(err) = copy_outcome {
        println!("{} Failed to preserve permissions for {}: {}",
                 "Warning".yellow(),
                 src.description().bold(),
                 err
      );
    }
    Ok(true)
}

pub fn sync_entries(src: &Entry, dest: &Entry)  -> io::Result<(bool)> {
    if is_link(&src.path())? {
        return copy_link(&src, &dest);
    }
    let more_recent = more_recent_than(&src, &dest)?;
    if more_recent {
        return copy_entry(&src, &dest);
    }
    Ok(false)
}


#[cfg(test)]
mod tests {

extern crate tempdir;
use self::tempdir::TempDir;

use std;
use std::error::Error;
use std::os::unix;
use std::path::Path;
use std::path::PathBuf;
use std::fs::File;
use std::io::prelude::*;

use super::Entry;
use super::copy_link;


fn create_file(path: &Path) {
    let mut out = File::create(path).expect(&format!("could not open {:?} for writing", path));
    out.write_all(b"").expect("could not write old test");
}

fn create_link(src: &str, dest: &Path) {
    unix::fs::symlink(&src, &dest).expect(
        &format!("could not link {:?} -> {:?}",
                src, dest));
}

fn assert_links_to(src: &str, dest: &Path) {
    let link = std::fs::read_link(dest).expect(
        &format!("could not read link {:?}", src));
    assert_eq!(link.to_string_lossy(), src);
}

fn setup_copy_test(tmp_path: &Path) -> PathBuf {
    let src = &tmp_path.join("src");
    create_file(&src);
    let src_link = &tmp_path.join("src_link");
    create_link("src", &src_link);
    src_link.to_path_buf()
}

#[test]
fn copy_link_dest_does_not_exist() {
    let tmp_dir = TempDir::new("test-rusync-fsops").expect("failed to create temp dir");
    let tmp_path = tmp_dir.path();
    let src_link = setup_copy_test(tmp_path);

    let new_link = &tmp_path.join("new");
    copy_link(&Entry::new(String::from("src"), &src_link), &Entry::new(String::from("dest"), &new_link)).expect("");
    assert_links_to("src", &new_link);
}

#[test]
fn copy_link_dest_is_a_broken_link() {
    let tmp_dir = TempDir::new("test-rusync-fsops").expect("failed to create temp dir");
    let tmp_path = tmp_dir.path();
    let src_link = setup_copy_test(tmp_path);

    let broken_link = &tmp_path.join("broken");
    create_link("no-such-file", &broken_link);
    copy_link(&Entry::new(String::from("src_link"), &src_link), &Entry::new(String::from("broken"), &broken_link)).expect("");
    assert_links_to("src", &broken_link);
}

#[test]
fn copy_link_dest_doest_not_point_to_correct_location() {
    let tmp_dir = TempDir::new("test-rusync-fsops").expect("failed to create temp dir");
    let tmp_path = tmp_dir.path();
    let src_link = setup_copy_test(tmp_path);

    let old_dest = &tmp_path.join("old");
    create_file(&old_dest);
    let existing_link = tmp_path.join("existing");
    create_link("old", &existing_link);
    copy_link(&Entry::new(String::from("src_link"), &src_link), &Entry::new(String::from("old"), &existing_link)).expect("");
    assert_links_to("src", &existing_link);
}

#[test]
fn copy_link_dest_is_a_regular_file() {
    let tmp_dir = TempDir::new("test-rusync-fsops").expect("failed to create temp dir");
    let tmp_path = tmp_dir.path();
    let src_link = setup_copy_test(tmp_path);

    let existing_file = tmp_path.join("existing");
    create_file(&existing_file);
    let outcome = copy_link(&Entry::new(String::from("src_link"), &src_link), &Entry::new(String::from("existing"), &existing_file));
    assert!(outcome.is_err());
    let err = outcome.err().unwrap();
    let desc = err.description();
    assert!(desc.contains("existing"));
}

}
