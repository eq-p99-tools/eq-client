//! Classic WLD boundary regions, independent of server destinations, and
//! the regions the zone fills with water or lava.
use std::collections::BTreeMap;

use glam::Vec3;
use libeq::wld::parser::{FragmentRef, Region, WldDoc, WorldTree, Zone};

use crate::LoadError;

#[derive(Clone, Debug)]
struct Node {
    normal: [f32; 3],
    distance: f32,
    region: Option<usize>,
    front: Option<usize>,
    back: Option<usize>,
}

/// Destination metadata encoded by a local WLD boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoneLine {
    /// Lookup number in the current server-provided zone-point table.
    Reference(u32),
    /// Older assets embed a destination directly.
    Absolute {
        /// Destination zone ID.
        zone_id: u16,
        /// Destination in world coordinates, as servers place spawns.
        position: [f32; 3],
        /// Facing, including the protocol's preserve-facing sentinel.
        heading: f32,
    },
}

/// What fills a region of the zone, read as `EQEmu` reads it for its water
/// maps (zone-utilities' `awater`, `water_map.cpp`): a region named `WT...`
/// is water and `LA...` lava, and one named for none of the kinds there is
/// read the same way by its user data. The server's own test for liquid is
/// water or lava (`WaterMap::InLiquid`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Liquid {
    /// Water.
    Water,
    /// Lava.
    Lava,
}

/// The prefixes `EQEmu`'s water maps read a region's name by before its
/// user data: water, lava, zone line, player-versus-player, slime, ice and
/// `VWA` regions.
const REGION_KINDS: [&str; 7] = ["WT", "LA", "DRNTP", "DRP_", "SL", "DRN", "VWA"];

/// Spatial lookup for zone-line references in classic WLD assets, and for
/// the regions filled with water or lava, in the renderer's frame like the
/// rest of a zone's geometry.
///
/// Numbers refer to the server's zone-point table, not destination zone IDs.
/// Unrecognized or malformed tags intentionally produce no route.
#[derive(Clone, Debug, Default)]
pub struct ZoneRegions {
    nodes: Vec<Node>,
    routes: BTreeMap<usize, ZoneLine>,
    liquids: BTreeMap<usize, Liquid>,
}

impl ZoneRegions {
    /// Loads the spatial tree and zone-line tags from an already parsed world.
    ///
    /// # Errors
    /// Rejects invalid tree references, non-finite planes and ambiguous regions.
    pub fn from_doc(doc: &WldDoc) -> Result<Self, LoadError> {
        let mut trees = doc.fragment_iter::<WorldTree>();
        let Some(tree) = trees.next() else {
            return Ok(Self::default());
        };
        if trees.next().is_some() {
            return Err(invalid("multiple world trees"));
        }
        let region_count = doc.fragment_iter::<Region>().count();
        let nodes = tree
            .world_nodes
            .iter()
            .map(|node| {
                let normal = [node.normal.0, node.normal.1, node.normal.2];
                if !normal.iter().all(|v| v.is_finite()) || !node.split_distance.is_finite() {
                    return Err(invalid("non-finite BSP plane"));
                }
                Ok(Node {
                    // The conversion keeps lengths, so a plane's distance stays.
                    normal: eq_client_axes::from_wld(Vec3::from_array(normal)).to_array(),
                    distance: node.split_distance,
                    region: index(&node.region, region_count)?,
                    front: index(&node.front_tree, tree.world_nodes.len())?,
                    back: index(&node.back_tree, tree.world_nodes.len())?,
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        let mut routes = BTreeMap::new();
        let mut liquids = BTreeMap::new();
        for zone in doc.fragment_iter::<Zone>() {
            let name = doc.get_string(zone.name_reference).unwrap_or_default();
            // As the water maps mark them, the last list naming a region
            // decides what fills it. A region past the tree fills nothing,
            // and a list naming one does not cost the zone its other regions.
            let liquid = liquid_tag(name, &zone.user_data);
            let listed = zone
                .regions
                .iter()
                .filter_map(|region| usize::try_from(*region).ok())
                .filter(|region| *region < region_count);
            for region in listed {
                match liquid {
                    Some(liquid) => liquids.insert(region, liquid),
                    None => liquids.remove(&region),
                };
            }
            let tag = if zone.user_data.is_empty() {
                name
            } else {
                &zone.user_data
            };
            let Some(route) = zone_line_tag(tag) else {
                continue;
            };
            for region in &zone.regions {
                // Zone lists are zero-based; tree leaf references are one-based.
                let region = usize::try_from(*region).map_err(|_| invalid("region overflow"))?;
                if region >= region_count {
                    return Err(invalid("zone-line region out of range"));
                }
                if routes.insert(region, route).is_some_and(|old| old != route) {
                    return Err(invalid("conflicting zone-line references"));
                }
            }
        }
        Ok(Self {
            nodes,
            routes,
            liquids,
        })
    }

    /// Finds a boundary at a position in the renderer's frame.
    ///
    /// A point exactly on a split plane, a broken tree, or a cycle yields no route.
    pub fn zone_line_at(&self, position: [f32; 3]) -> Option<ZoneLine> {
        self.routes.get(&self.region_at(position)?).copied()
    }

    /// What fills the zone at a position in the renderer's frame: water,
    /// lava, or neither.
    ///
    /// A point exactly on a split plane, a broken tree, or a cycle is in
    /// neither.
    pub fn liquid_at(&self, position: [f32; 3]) -> Option<Liquid> {
        self.liquids.get(&self.region_at(position)?).copied()
    }

    /// The leaf region holding a position in the renderer's frame.
    fn region_at(&self, position: [f32; 3]) -> Option<usize> {
        if !position.iter().all(|v| v.is_finite()) {
            return None;
        }
        let mut cursor = 0;
        for _ in 0..self.nodes.len() {
            let node = self.nodes.get(cursor)?;
            if let Some(region) = node.region {
                return Some(region);
            }
            let distance = position
                .iter()
                .zip(node.normal)
                .fold(node.distance, |sum, (p, n)| sum + p * n);
            if !distance.is_finite() || distance == 0.0 {
                return None;
            }
            cursor = if distance > 0.0 {
                node.front
            } else {
                node.back
            }?;
        }
        None
    }

    /// Returns a point inside the first boundary crossed by a movement segment,
    /// in the renderer's frame. An already occupied boundary is ignored so
    /// admission does not trigger zoning.
    #[allow(clippy::cast_possible_truncation)] // Interpolation stays between finite f32 endpoints.
    pub fn zone_line_entry(&self, start: [f32; 3], end: [f32; 3]) -> Option<[f32; 3]> {
        if self.routes.is_empty()
            || !start.iter().chain(&end).all(|v| v.is_finite())
            || self.zone_line_at(start).is_some()
        {
            return None;
        }
        // Every leaf transition crosses a BSP plane. Partitioning at all plane
        // intersections avoids a sampling stride that could skip narrow regions.
        let mut cuts = vec![0.0_f64, 1.0];
        for node in &self.nodes {
            if node.region.is_some() {
                continue;
            }
            let distance = |point: [f32; 3]| {
                point
                    .into_iter()
                    .zip(node.normal)
                    .fold(f64::from(node.distance), |sum, (p, n)| {
                        sum + f64::from(p) * f64::from(n)
                    })
            };
            let a = distance(start);
            let b = distance(end);
            let t = a / (a - b);
            if t.is_finite() && t > 0.0 && t < 1.0 {
                cuts.push(t);
            }
        }
        cuts.sort_unstable_by(f64::total_cmp);
        cuts.dedup();
        for pair in cuts.windows(2) {
            let t = f64::midpoint(pair[0], pair[1]);
            let point = std::array::from_fn(|axis| {
                (f64::from(start[axis]) + (f64::from(end[axis]) - f64::from(start[axis])) * t)
                    as f32
            });
            if self.zone_line_at(point).is_some() {
                return Some(point);
            }
        }
        None
    }

    /// Number of BSP leaf regions linked to numbered zone points.
    pub fn boundary_region_count(&self) -> usize {
        self.routes.len()
    }

    /// Number of BSP leaf regions filled with water or lava.
    pub fn liquid_region_count(&self) -> usize {
        self.liquids.len()
    }
}

fn invalid(message: &str) -> LoadError {
    LoadError::ParseWorld(message.into())
}

fn index<T>(reference: &FragmentRef<T>, len: usize) -> Result<Option<usize>, LoadError> {
    match reference {
        FragmentRef::Index(value, _) => {
            let value = usize::try_from(*value).map_err(|_| invalid("BSP index overflow"))?;
            if value == 0 || value > len {
                return Err(invalid("BSP reference out of range"));
            }
            Ok(Some(value - 1))
        }
        FragmentRef::Name(name, _) if name.to_bytes() == [0; 4] => Ok(None),
        FragmentRef::Name(..) => Err(invalid("named BSP reference")),
    }
}

/// What fills a region with this name and user data, as `EQEmu`'s water
/// maps read them: by the name when it starts with any kind's prefix, else
/// by the user data. Their comparison is exact, so case matters.
fn liquid_tag(name: &str, user_data: &str) -> Option<Liquid> {
    let tag = if REGION_KINDS.iter().any(|kind| name.starts_with(kind)) {
        name
    } else {
        user_data
    };
    if tag.starts_with("WT") {
        Some(Liquid::Water)
    } else if tag.starts_with("LA") {
        Some(Liquid::Lava)
    } else {
        None
    }
}

/// The 255 destination marker means the next six digits name a server route.
fn zone_line_tag(tag: &str) -> Option<ZoneLine> {
    let tag = tag.to_ascii_uppercase();
    if !["DRNTP", "WTNTP", "LANTP"]
        .iter()
        .any(|prefix| tag.starts_with(prefix))
    {
        return None;
    }
    let zone_id: u16 = tag.get(5..10)?.parse().ok()?;
    if zone_id == 255 {
        let number = tag.get(10..16)?;
        return number
            .bytes()
            .all(|value| value.is_ascii_digit())
            .then(|| number.parse().ok().map(ZoneLine::Reference))
            .flatten();
    }
    if zone_id == 0 {
        return None;
    }
    let decimal = |start, end| -> Option<f32> {
        let value = tag.get(start..end)?;
        let digits = value.strip_prefix('-').unwrap_or(value);
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        value.parse().ok()
    };
    let wld = Vec3::new(decimal(10, 16)?, decimal(16, 22)?, decimal(22, 28)?);
    Some(ZoneLine::Absolute {
        zone_id,
        position: eq_client_axes::wld_to_world(wld).to_array(),
        heading: decimal(28, 31)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_tags_do_not_guess_absolute_or_malformed_destinations() {
        assert_eq!(
            zone_line_tag("DRNTP00255000007_ZONE"),
            Some(ZoneLine::Reference(7))
        );
        assert_eq!(
            zone_line_tag("wtntp0025500000800000000000000_ZONE"),
            Some(ZoneLine::Reference(8))
        );
        for tag in [
            "DRNTP_ZONE",
            "DRNTP00008000007_ZONE",
            "DRNTP0025500000x_ZONE",
            "WT_ZONE",
            "DRNTP00255ééé",
        ] {
            assert_eq!(zone_line_tag(tag), None);
        }
        assert_eq!(
            zone_line_tag("DRNTP00042-00012000034000056999_ZONE"),
            Some(ZoneLine::Absolute {
                zone_id: 42,
                // Written in WLD order, the world's Y first.
                position: [34.0, -12.0, 56.0],
                heading: 999.0
            })
        );
    }

    fn leaf(region: usize) -> Node {
        Node {
            normal: [0.0; 3],
            distance: 0.0,
            region: Some(region),
            front: None,
            back: None,
        }
    }

    fn tree() -> ZoneRegions {
        ZoneRegions {
            nodes: vec![
                Node {
                    normal: [1.0, 0.0, 0.0],
                    distance: -10.0,
                    region: None,
                    front: Some(1),
                    back: Some(2),
                },
                leaf(0),
                leaf(1),
            ],
            routes: BTreeMap::from([(0, ZoneLine::Reference(7))]),
            liquids: BTreeMap::new(),
        }
    }

    #[test]
    fn segment_detects_a_thin_boundary_that_both_endpoints_miss() {
        let regions = ZoneRegions {
            nodes: vec![
                Node {
                    normal: [1.0, 0.0, 0.0],
                    distance: -10.0,
                    region: None,
                    front: Some(1),
                    back: Some(2),
                },
                Node {
                    normal: [1.0, 0.0, 0.0],
                    distance: -10.125,
                    region: None,
                    front: Some(2),
                    back: Some(3),
                },
                leaf(1),
                leaf(0),
            ],
            routes: BTreeMap::from([(0, ZoneLine::Reference(7))]),
            liquids: BTreeMap::new(),
        };
        let start = [9.0, 0.0, 0.0];
        let end = [11.0, 0.0, 0.0];
        assert!(regions.zone_line_at(start).is_none());
        assert!(regions.zone_line_at(end).is_none());
        for (from, to) in [(start, end), (end, start)] {
            let entry = regions.zone_line_entry(from, to).unwrap();
            assert!(entry[0] > 10.0 && entry[0] < 10.125);
            assert_eq!(regions.zone_line_at(entry), Some(ZoneLine::Reference(7)));
            assert!(regions.zone_line_entry(entry, to).is_none());
        }
        assert!(regions.zone_line_entry(start, start).is_none());
        assert!(regions.zone_line_entry([f32::NAN, 0.0, 0.0], end).is_none());
    }

    #[test]
    fn water_and_lava_are_read_as_the_servers_water_maps_read_them() {
        assert_eq!(liquid_tag("WT_ZONE", ""), Some(Liquid::Water));
        assert_eq!(liquid_tag("LA_ZONE", ""), Some(Liquid::Lava));
        // A zone line under water is water too.
        assert_eq!(liquid_tag("WTNTP00255000007_ZONE", ""), Some(Liquid::Water));
        // A name of another kind decides, whatever the user data says.
        assert_eq!(liquid_tag("DRNTP00255000007_ZONE", "WT"), None);
        assert_eq!(liquid_tag("DRP_ZONE", "LA"), None);
        assert_eq!(liquid_tag("SLIME_ZONE", "WT"), None);
        // Any other name leaves it to the user data.
        assert_eq!(liquid_tag("Z0001_ZONE", "WTN__"), Some(Liquid::Water));
        assert_eq!(liquid_tag("Z0001_ZONE", "LA"), Some(Liquid::Lava));
        assert_eq!(liquid_tag("Z0001_ZONE", "VWA"), None);
        assert_eq!(liquid_tag("Z0001_ZONE", ""), None);
        // The comparison is exact, as the server's is.
        assert_eq!(liquid_tag("wt_zone", ""), None);
    }

    #[test]
    fn a_point_is_in_the_liquid_of_the_region_holding_it() {
        let mut regions = tree();
        regions.liquids = BTreeMap::from([(1, Liquid::Water)]);
        assert_eq!(regions.liquid_at([9.0, 50.0, 1.0]), Some(Liquid::Water));
        assert_eq!(regions.liquid_at([11.0, 50.0, 1.0]), None);
        // On the plane, or nowhere, is in neither.
        assert_eq!(regions.liquid_at([10.0, 50.0, 1.0]), None);
        assert_eq!(regions.liquid_at([f32::NAN, 0.0, 0.0]), None);
        assert_eq!(regions.liquid_region_count(), 1);
        // The zone line keeps its route.
        assert_eq!(
            regions.zone_line_at([11.0, 50.0, 1.0]),
            Some(ZoneLine::Reference(7))
        );
    }

    #[test]
    fn positive_plane_side_enters_boundary_and_invalid_queries_fail_closed() {
        let mut regions = tree();
        assert_eq!(
            regions.zone_line_at([11.0, 50.0, 1.0]),
            Some(ZoneLine::Reference(7))
        );
        assert_eq!(regions.zone_line_at([9.0, 50.0, 1.0]), None);
        assert_eq!(regions.zone_line_at([10.0, 50.0, 1.0]), None);
        assert_eq!(regions.zone_line_at([f32::NAN, 0.0, 0.0]), None);
        regions.nodes[0].front = Some(0);
        assert_eq!(regions.zone_line_at([11.0, 0.0, 0.0]), None);
        regions.nodes[0].front = Some(99);
        assert_eq!(regions.zone_line_at([11.0, 0.0, 0.0]), None);
    }
}
