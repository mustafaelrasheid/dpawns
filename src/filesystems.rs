use std::os::unix::fs::symlink;
use std::os::unix::prelude::PermissionsExt;
use std::fs::{
    create_dir_all,
    remove_dir_all,
    set_permissions,
    remove_file
};
use std::io::ErrorKind;
use nix::mount::{mount, umount, MsFlags};
use crate::utils::delay;

fn set_root() {
    set_permissions(
        "/",
        PermissionsExt::from_mode(0o755)
    ).unwrap_or_else(|e| {
        eprintln!("Failed to set permissions on / to 0o755 due to {}", e);
    });
}

fn mount_filesystems(filesystems: &[(String, String, String, u32, MsFlags)]) {
    for (source, target, fstype, perm, flags) in filesystems{
        create_dir_all(target)
            .unwrap_or_else(|e| {
                eprintln!(
                    "Failed to create directory:\"{}\" due to {}.",
                    target, e);
            });
        mount(
            Some(source.as_str()),
            target.as_str(),
            Some(fstype.as_str()),
            *flags,
            None::<&str>
        ).unwrap_or_else(|e| {
            eprintln!(
                "Failed to mount filesystem {} at \"{}\" due to {}",
                source, target, e);
        });
        set_permissions(
            target,
            PermissionsExt::from_mode(*perm)
        ).unwrap_or_else(|e| {
            eprintln!("Failed to set permissions on {} due to {}", target, e);
        });
    }
}

fn umount_filesystems(filesystems: &[(String, String, String, u32, MsFlags)]) {
    for (source, target, _fstype, _perm, _flags) in filesystems
        .into_iter().rev() {
        umount(target.as_str()).unwrap_or_else(|e| {
            eprintln!(
                "Failed to unmount {} at \"{}\" due to {}",
                source, target, e);
        });
        remove_dir_all(target).unwrap_or_else(|e| {
            eprintln!("Failed to Delete {} due to {}",
                target, e);
        });
    }
}

fn set_symlinks(syslinks: &[(String, String)]) {
    for (origin, destination) in syslinks {
        symlink(origin, destination).unwrap_or_else(|e| {
            match e.kind() {
                ErrorKind::AlreadyExists => (),
                _ => {
                    eprintln!(
                        "Failed to syslink {} to {} due to {}",
                        origin, destination, e);
                }
            }
        });
    }
}

fn cleanup_symlinks(syslinks: &[(String, String)]) {
    for (_, destination) in syslinks.into_iter().rev() {
        remove_file(destination).unwrap_or_else(|e| {
            match e.kind() {
                ErrorKind::NotFound => (),
                _ => {
                    eprintln!(
                        "Failed to remove {} symlink: {}",
                        destination, e);
                }
            }
        });
    }
}

fn get_filesystems() -> Vec<(String, String, String, u32, MsFlags)> {
    let filesystems = vec![
        ("proc".to_string(), "/proc".to_string(), "proc".to_string(),
         0o755,  MsFlags::empty()),
        ("sysfs".to_string(), "/sys".to_string(), "sysfs".to_string(),
         0o555,  MsFlags::empty()),
        ("devtmpfs".to_string(), "/dev".to_string(), "devtmpfs".to_string(),
         0o755,  MsFlags::empty()),
        ("devpts".to_string(), "/dev/pts".to_string(), "devpts".to_string(),
         0o755,  MsFlags::MS_NOSUID | MsFlags::MS_NOEXEC),
        ("tmpfs".to_string(), "/tmp".to_string(), "tmpfs".to_string(),
         0o1777, MsFlags::MS_NOSUID | MsFlags::MS_NODEV | MsFlags::MS_NOEXEC),
        ("tmpfs".to_string(), "/run".to_string(), "tmpfs".to_string(),
         0o755,  MsFlags::MS_NOSUID | MsFlags::MS_NODEV),
        ("tmpfs".to_string(), "/lib64".to_string(), "tmpfs".to_string(),
         0o755,  MsFlags::empty()),
    ];

    return filesystems
}

fn get_symlinks() -> Vec<(String, String)> {
    let symlinks = vec![
        ("/lib/ld-linux-x86-64.so.2".to_string(),
         "/lib64/ld-linux-x86-64.so.2".to_string()),
    ];

    return symlinks;
}

pub fn setup() {
    let filesystems = get_filesystems();
    let symlinks = get_symlinks();

    set_root();
    mount_filesystems(&filesystems);
    set_symlinks(&symlinks);
}

pub fn close() {
    let filesystems = get_filesystems();
    let symlinks = get_symlinks();

    cleanup_symlinks(&symlinks);
    delay();
    umount_filesystems(&filesystems);
}
