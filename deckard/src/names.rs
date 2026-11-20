use crate::config::NameConfig;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Filename matches stored as one membership list per name.
/// Use the path and group accessors on [`FileIndex`](crate::index::FileIndex) to read results.
#[derive(Debug, Default, Clone)]
pub struct NameMatches {
    pub(crate) groups: BTreeMap<Arc<[u8]>, BTreeSet<Arc<PathBuf>>>,
    pub(crate) by_path: HashMap<Arc<PathBuf>, Arc<[u8]>>,
}

impl NameMatches {
    pub(crate) fn insert_group(&mut self, group: &[(Vec<u8>, &Arc<PathBuf>)]) {
        if group.len() < 2 {
            return;
        }
        let name: Arc<[u8]> = group[0].0.as_slice().into();
        let mut members = BTreeSet::new();
        for (_, path) in group {
            members.insert(Arc::clone(path));
            self.by_path.insert(Arc::clone(path), Arc::clone(&name));
        }
        self.groups.insert(name, members);
    }

    pub(crate) fn members(&self, path: &PathBuf) -> Option<&BTreeSet<Arc<PathBuf>>> {
        self.groups.get(self.by_path.get(path)?)
    }

    // Return the last remaining path when removing a member dissolves the group.
    pub(crate) fn remove(&mut self, path: &PathBuf) -> Option<Arc<PathBuf>> {
        let name = self.by_path.remove(path)?;
        let members = self.groups.get_mut(&name)?;
        members.remove(path);
        if members.len() == 1 {
            let last = members.pop_first()?;
            self.groups.remove(&name);
            self.by_path.remove(&last);
            return Some(last);
        }
        None
    }
}

impl NameConfig {
    // Keep opaque filename bytes in the key; use lossy conversion only for display.
    pub(crate) fn filtered_name(&self, path: &Path) -> Option<Vec<u8>> {
        let stem = path.file_stem()?.as_encoded_bytes();
        let (prefix, title) = if self.ignore_track_numbers {
            unnumbered_title(stem)
        } else {
            (&[][..], stem)
        };
        let mut key = Vec::with_capacity(stem.len());
        for part in [prefix, title] {
            for chunk in part.utf8_chunks() {
                let mut folded = unicase::UniCase::new(chunk.valid()).to_folded_case();
                if self.ignore_separators {
                    folded.retain(|c| !separator(c));
                }
                key.extend_from_slice(folded.as_bytes());
                key.extend_from_slice(chunk.invalid());
            }
        }
        (!key.is_empty()).then_some(key)
    }
}

fn separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '-' | '_' | '.')
}

fn strip_track_number(title: &[u8]) -> Option<&[u8]> {
    let digits = title.iter().take_while(|b| b.is_ascii_digit()).count();
    let rest = &title[digits..];
    let text = rest.utf8_chunks().next()?.valid();
    let delimiters = text.len() - text.trim_start_matches(separator).len();
    (digits > 0 && delimiters > 0 && delimiters < rest.len()).then(|| &rest[delimiters..])
}

// Split after the final spaced hyphen, retaining the separator in the prefix.
fn split_title(stem: &[u8]) -> (&[u8], &[u8]) {
    let boundary = b" - ";
    let start = stem
        .windows(boundary.len())
        .rposition(|w| w == boundary)
        .map_or(0, |i| i + boundary.len());
    stem.split_at(start)
}

fn unnumbered_title(stem: &[u8]) -> (&[u8], &[u8]) {
    let (prefix, title) = split_title(stem);
    // An explicit track field takes precedence even when the title starts with digits.
    if let Some(before) = prefix.strip_suffix(b" - ") {
        let (artist, number) = split_title(before);
        if !title.is_empty() && !number.is_empty() && number.iter().all(u8::is_ascii_digit) {
            return (artist, title);
        }
    }
    if let Some(clean) = strip_track_number(title) {
        return (prefix, clean);
    }
    // The title itself may contain a spaced hyphen, as in "04 Thimblewake - Live".
    if !prefix.is_empty()
        && let Some(clean) = strip_track_number(stem)
    {
        return (&[], clean);
    }
    (prefix, title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_groups_track_numbers() {
        let config = NameConfig::default();
        let key = |name: &str| config.filtered_name(Path::new(name));
        for track in [
            "04 Thimblewake.wav",
            "12 THIMBLEWAKE.flac",
            "4. thimblewake.mp3",
            "04 - Thimblewake",
        ] {
            assert_eq!(key(track), key("thimblewake"));
        }
        assert_eq!(
            key("Avenquill - Paper Moonsill - 04 Thimblewake.wav"),
            key("Avenquill - Paper Moonsill - Thimblewake.flac")
        );
        assert_eq!(
            key("Artist - 04 - Thimblewake.wav"),
            key("Artist - Thimblewake.flac")
        );
        assert_eq!(
            key("04 Thimblewake - Live.wav"),
            key("Thimblewake - Live.flac")
        );
        assert_ne!(key("Artist - Part 1.wav"), key("Artist - Part 2.wav"));
        assert_eq!(key("Floßwisp.wav"), key("FLOSSWISP.mp3"));
        assert_ne!(key("3702.wav"), key("4803.wav"));
        assert_ne!(
            key("Artist - 12 Mothbells - Thimblewake.wav"),
            key("Artist - Mothbells - Thimblewake.wav")
        );
        assert_ne!(key("track.demo.wav"), key("track.wav"));
    }

    #[test]
    fn explicit_track_number_preserves_numbered_title() {
        let config = NameConfig::default();
        let key = |name: &str| config.filtered_name(Path::new(name));
        let track = key("Avenquill - 04 - 37 Mothbells.wav");
        assert_eq!(track, key("Avenquill - 05 - 37 Mothbells.flac"));
        assert_ne!(track, key("Avenquill - 04 - 38 Mothbells.wav"));
    }

    #[cfg(unix)]
    #[test]
    fn name_groups_opaque_bytes() {
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        let config = NameConfig {
            ignore_track_numbers: true,
            ignore_separators: true,
        };
        let key = |name: &[u8]| config.filtered_name(Path::new(OsStr::from_bytes(name)));
        assert_eq!(key(b"04 Reed-Wisp\xff.wav"), key(b"reedwisp\xff.flac"));
        assert_ne!(key(b"ReedWisp\xfe.wav"), key(b"ReedWisp\xff.wav"));
        assert_ne!(key(b"ReedWisp\xff.wav"), key("ReedWisp�.wav".as_bytes()));
    }

    #[test]
    fn name_groups_separators() {
        for tracks in [true, false] {
            for separators in [true, false] {
                let config = NameConfig {
                    ignore_track_numbers: tracks,
                    ignore_separators: separators,
                };
                let key = |name: &str| config.filtered_name(Path::new(name));
                for name in [
                    "Reed Wisp.wav",
                    "REED-WISP.flac",
                    "reed_wisp.mp3",
                    "Reed.Wisp.wav",
                ] {
                    assert_eq!(key(name) == key("ReedWisp"), separators);
                }
                assert_eq!(key("04 Thimblewake.wav") == key("thimblewake.flac"), tracks);
                assert_eq!(key("-_.wav").is_none(), separators);
                assert_ne!(key("C++.wav"), key("C.wav"));
            }
        }
    }
}
