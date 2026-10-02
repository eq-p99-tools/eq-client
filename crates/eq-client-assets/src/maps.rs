//! The zone maps the installed client keeps in its `maps` folder: a zone's
//! base map (`<zone>.txt`) and up to three layers (`<zone>_1.txt` to
//! `<zone>_3.txt`). Each is a list of lines (`L x1, y1, z1, x2, y2, z2, r,
//! g, b`) and labelled points (`P x, y, z, r, g, b, size, label`, the
//! label's spaces written as `_`). A map's x and y are the world's negated,
//! so they grow to the east and to the south.
use std::path::Path;

/// One line of a zone map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapLine {
    /// Where it starts: map x, map y and height.
    pub from: [f32; 3],
    /// Where it ends.
    pub to: [f32; 3],
    /// Its colour.
    pub color: [u8; 3],
    /// The file it came from: 0 for the base map, 1 to 3 for a layer.
    pub layer: u8,
}

/// One labelled point of a zone map.
#[derive(Clone, Debug, PartialEq)]
pub struct MapLabel {
    /// Where it is: map x, map y and height.
    pub at: [f32; 3],
    /// Its colour.
    pub color: [u8; 3],
    /// How large its words are, 0 to 3.
    pub size: u8,
    /// Its words, spaces restored.
    pub text: String,
    /// The file it came from: 0 for the base map, 1 to 3 for a layer.
    pub layer: u8,
}

/// A zone's map: its lines and labels from every file the installation has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ZoneMap {
    /// The lines, in file order.
    pub lines: Vec<MapLine>,
    /// The labelled points, in file order.
    pub labels: Vec<MapLabel>,
}

impl ZoneMap {
    /// The zone's map from the installation's `maps` folder, or None when it
    /// has no file for the zone. The server names the zone, so a name that
    /// could reach outside the folder has none.
    #[must_use]
    pub fn load(eq_directory: &Path, zone: &str) -> Option<Self> {
        let zone = crate::zone_name(zone).ok()?;
        let folder = eq_directory.join("maps");
        let mut map = Self::default();
        let mut found = false;
        for layer in 0..=3u8 {
            let name = if layer == 0 {
                format!("{zone}.txt")
            } else {
                format!("{zone}_{layer}.txt")
            };
            if let Ok(bytes) = std::fs::read(folder.join(name)) {
                found = true;
                map.read(&String::from_utf8_lossy(&bytes), layer);
            }
        }
        found.then_some(map)
    }

    /// Adds the lines and labels of one file's text; lines it cannot read
    /// are skipped.
    pub fn read(&mut self, text: &str, layer: u8) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix('L') {
                let numbers: Vec<f32> = rest
                    .split(',')
                    .filter_map(|part| part.trim().parse().ok())
                    .collect();
                if let [x1, y1, z1, x2, y2, z2, red, green, blue] = numbers[..] {
                    self.lines.push(MapLine {
                        from: [x1, y1, z1],
                        to: [x2, y2, z2],
                        color: [channel(red), channel(green), channel(blue)],
                        layer,
                    });
                }
            } else if let Some(rest) = line.strip_prefix('P') {
                let mut parts: Vec<&str> = rest.splitn(8, ',').map(str::trim).collect();
                let (Some(text), 7) = (parts.pop(), parts.len()) else {
                    continue;
                };
                let numbers: Option<Vec<f32>> =
                    parts.iter().map(|part| part.parse().ok()).collect();
                let Some([x, y, z, red, green, blue, size]) =
                    numbers.and_then(|numbers| <[f32; 7]>::try_from(numbers).ok())
                else {
                    continue;
                };
                self.labels.push(MapLabel {
                    at: [x, y, z],
                    color: [channel(red), channel(green), channel(blue)],
                    size: channel(size).min(3),
                    text: text.replace('_', " "),
                    layer,
                });
            }
        }
    }

    /// The smallest and largest map x and y its lines and labels reach.
    #[must_use]
    pub fn bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        let points = self
            .lines
            .iter()
            .flat_map(|line| [line.from, line.to])
            .chain(self.labels.iter().map(|label| label.at));
        points.fold(None, |bounds, [x, y, _]| {
            let ([min_x, min_y], [max_x, max_y]) = bounds.unwrap_or(([x, y], [x, y]));
            Some(([min_x.min(x), min_y.min(y)], [max_x.max(x), max_y.max(y)]))
        })
    }
}

/// A colour channel or size written as a number.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Clamped first.
fn channel(value: f32) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_labels_read_with_their_layers() {
        let mut map = ZoneMap::default();
        map.read(
            "L 1.5, -2.0, 0.0,  3.0, 4.0, 1.0,  64, 64, 64\n\
             P 10.0, 20.0, -1.0,  0, 0, 0,  3,  Example_(Spells)\n\
             L broken\n",
            0,
        );
        map.read("L 0, 0, 0, -5, 0, 0, 255, 0, 0\n", 2);
        assert_eq!(
            map.lines,
            [
                MapLine {
                    from: [1.5, -2.0, 0.0],
                    to: [3.0, 4.0, 1.0],
                    color: [64, 64, 64],
                    layer: 0,
                },
                MapLine {
                    from: [0.0, 0.0, 0.0],
                    to: [-5.0, 0.0, 0.0],
                    color: [255, 0, 0],
                    layer: 2,
                }
            ]
        );
        assert_eq!(map.labels.len(), 1);
        assert_eq!(map.labels[0].text, "Example (Spells)");
        assert_eq!(map.labels[0].size, 3);
        assert_eq!(map.bounds(), Some(([-5.0, -2.0], [10.0, 20.0])));
    }

    #[test]
    fn a_zone_without_files_has_no_map() {
        let folder = std::env::temp_dir().join("eq-client-maps-test-none");
        assert_eq!(ZoneMap::load(&folder, "nowhere"), None);
        assert_eq!(ZoneMap::load(&folder, "../outside"), None);
    }
}
