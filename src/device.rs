use std::{
    path::Path,
    time::{Duration, Instant},
};
use sysinfo::Disks;

const REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const BYTES_PER_GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct Storage {
    pub total_gib: f64,
    pub available_gib: f64,
}

pub struct Monitor {
    disks: Disks,
    storage: Storage,
    last_refresh: Instant,
}

impl Monitor {
    pub fn new(models_dir: &Path) -> Self {
        let disks = Disks::new_with_refreshed_list();
        let storage = storage_for_path(&disks, models_dir).unwrap_or_default();
        Self {
            disks,
            storage,
            last_refresh: Instant::now(),
        }
    }

    pub fn refresh_if_due(&mut self, models_dir: &Path) -> bool {
        if self.last_refresh.elapsed() < REFRESH_INTERVAL {
            return false;
        }
        self.disks.refresh(false);
        let next = storage_for_path(&self.disks, models_dir).unwrap_or_default();
        let changed = (next.available_gib - self.storage.available_gib).abs() >= 0.1
            || (next.total_gib - self.storage.total_gib).abs() >= 0.1;
        self.storage = next;
        self.last_refresh = Instant::now();
        changed
    }

    pub const fn storage(&self) -> Storage {
        self.storage
    }
}

fn storage_for_path(disks: &Disks, path: &Path) -> Option<Storage> {
    let index = best_mount_index(
        path,
        &disks
            .list()
            .iter()
            .map(|disk| disk.mount_point())
            .collect::<Vec<_>>(),
    )?;
    let disk = &disks.list()[index];
    Some(Storage {
        total_gib: disk.total_space() as f64 / BYTES_PER_GIB,
        available_gib: disk.available_space() as f64 / BYTES_PER_GIB,
    })
}

fn best_mount_index(path: &Path, mounts: &[&Path]) -> Option<usize> {
    mounts
        .iter()
        .enumerate()
        .filter(|(_, mount)| path.starts_with(mount))
        .max_by_key(|(_, mount)| mount.components().count())
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_uses_the_most_specific_mount_for_the_models_directory() {
        let mounts = [Path::new("/"), Path::new("/mnt/models"), Path::new("/tmp")];
        assert_eq!(
            best_mount_index(Path::new("/mnt/models/qwen"), &mounts),
            Some(1)
        );
        assert_eq!(
            best_mount_index(Path::new("/home/user/models"), &mounts),
            Some(0)
        );
    }
}
