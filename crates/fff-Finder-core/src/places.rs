//! Sidebar locations: standard favorites and mounted volumes, per platform.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Icon hint for a sidebar row; the GUI maps these to glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaceIcon {
    Applications,
    Desktop,
    Documents,
    Downloads,
    Pictures,
    Music,
    Movies,
    Home,
    Cloud,
    Drive,
    External,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
    pub icon: PlaceIcon,
    /// Removable volumes can be ejected.
    pub ejectable: bool,
}

impl Place {
    fn new(name: impl Into<String>, path: PathBuf, icon: PlaceIcon) -> Place {
        Place {
            name: name.into(),
            path,
            icon,
            ejectable: false,
        }
    }
}

/// Finder's default favorites that exist on this machine.
pub fn default_favorites() -> Vec<Place> {
    let mut places = Vec::new();
    #[cfg(target_os = "macos")]
    places.push(Place::new(
        "Applications",
        PathBuf::from("/Applications"),
        PlaceIcon::Applications,
    ));
    let candidates = [
        (dirs::document_dir(), "Documents", PlaceIcon::Documents),
        (dirs::download_dir(), "Downloads", PlaceIcon::Downloads),
        (dirs::desktop_dir(), "Desktop", PlaceIcon::Desktop),
        (dirs::picture_dir(), "Pictures", PlaceIcon::Pictures),
        (dirs::audio_dir(), "Music", PlaceIcon::Music),
        (
            dirs::video_dir(),
            if cfg!(target_os = "macos") {
                "Movies"
            } else {
                "Videos"
            },
            PlaceIcon::Movies,
        ),
    ];
    for (dir, name, icon) in candidates {
        if let Some(dir) = dir.filter(|d| d.is_dir()) {
            places.push(Place::new(name, dir, icon));
        }
    }
    places
}

/// Map an arbitrary favorite folder to a label and icon.
pub fn favorite_for(path: &Path) -> Place {
    default_favorites()
        .into_iter()
        .find(|p| p.path == path)
        .unwrap_or_else(|| {
            Place::new(
                crate::entry::display_name(path),
                path.to_path_buf(),
                PlaceIcon::Folder,
            )
        })
}

/// Home, cloud drive, and mounted volumes.
pub fn locations() -> Vec<Place> {
    let mut places = Vec::new();
    if let Some(home) = dirs::home_dir() {
        #[cfg(target_os = "macos")]
        {
            let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
            if icloud.is_dir() {
                places.push(Place::new("iCloud Drive", icloud, PlaceIcon::Cloud));
            }
        }
        places.push(Place::new(
            crate::entry::display_name(&home),
            home,
            PlaceIcon::Home,
        ));
    }
    places.extend(volumes());
    places
}

#[cfg(target_os = "macos")]
fn volumes() -> Vec<Place> {
    let mut out = Vec::new();
    let root_dev = device_id(Path::new("/"));
    let Ok(read) = std::fs::read_dir("/Volumes") else {
        return vec![Place::new("Macintosh HD", "/".into(), PlaceIcon::Drive)];
    };
    let mut boot_name = None;
    for entry in read.flatten() {
        let path = entry.path();
        let name = crate::entry::display_name(&path);
        // System volumes macOS mounts but Finder hides.
        const SYSTEM: [&str; 7] = [
            "Recovery",
            "Preboot",
            "VM",
            "Update",
            "xarts",
            "iSCPreboot",
            "Hardware",
        ];
        if name.starts_with('.') || SYSTEM.contains(&name.as_str()) {
            continue;
        }
        // The boot volume appears in /Volumes as a symlink to "/".
        if std::fs::read_link(&path).is_ok_and(|t| t == Path::new("/"))
            || (root_dev.is_some() && device_id(&path) == root_dev)
        {
            boot_name = Some(name);
            continue;
        }
        let mut place = Place::new(name, path, PlaceIcon::External);
        place.ejectable = true;
        out.push(place);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.insert(
        0,
        Place::new(
            boot_name.unwrap_or_else(|| "Macintosh HD".into()),
            "/".into(),
            PlaceIcon::Drive,
        ),
    );
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
fn volumes() -> Vec<Place> {
    let mut out = vec![Place::new("Computer", "/".into(), PlaceIcon::Drive)];
    let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else {
        return out;
    };
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(device), Some(mount)) = (fields.next(), fields.next()) else {
            continue;
        };
        let mount = mount.replace("\\040", " ");
        let user_mount = mount.starts_with("/media/")
            || mount.starts_with("/run/media/")
            || mount.starts_with("/mnt/");
        if !user_mount || !device.starts_with('/') {
            continue;
        }
        let path = PathBuf::from(&mount);
        let mut place = Place::new(crate::entry::display_name(&path), path, PlaceIcon::External);
        place.ejectable = true;
        out.push(place);
    }
    out
}

#[cfg(windows)]
fn volumes() -> Vec<Place> {
    let mut out = Vec::new();
    // A: and B: are skipped: probing an empty floppy drive can raise a system dialog.
    for letter in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        if root.exists() {
            let icon = if letter == b'C' {
                PlaceIcon::Drive
            } else {
                PlaceIcon::External
            };
            out.push(Place::new(
                format!("Local Disk ({}:)", letter as char),
                root,
                icon,
            ));
        }
    }
    out
}

#[cfg(unix)]
fn device_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

/// Breadcrumb components from the volume root down to `path`.
pub fn breadcrumbs(path: &Path) -> Vec<(String, PathBuf)> {
    let mut crumbs: Vec<(String, PathBuf)> = path
        .ancestors()
        .map(|p| (crate::entry::display_name(p), p.to_path_buf()))
        .collect();
    crumbs.reverse();
    #[cfg(target_os = "macos")]
    if let Some(first) = crumbs.first_mut()
        && first.1 == Path::new("/")
    {
        first.0 = volumes()
            .into_iter()
            .next()
            .map(|v| v.name)
            .unwrap_or_else(|| "Macintosh HD".into());
    }
    crumbs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breadcrumbs_root_first() {
        let home = dirs::home_dir().unwrap();
        let crumbs = breadcrumbs(&home);
        assert!(crumbs.len() >= 2);
        assert_eq!(crumbs.last().unwrap().1, home);
    }

    #[test]
    fn has_locations() {
        assert!(!locations().is_empty());
    }
}
