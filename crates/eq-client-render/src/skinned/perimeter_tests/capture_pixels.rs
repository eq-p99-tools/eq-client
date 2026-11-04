//! Readback checks inspect the actual saved PNG, not a replacement renderer.
//! Metadata supplies production layout/clip rectangles; generated source colors
//! supply independent pixel expectations away from nearest-texel boundaries.
use super::TAGS;
use bevy::prelude::*;
use serde_json::Value;
use std::path::Path;

const BACKGROUND: [u8; 3] = [32, 64, 96];
const NORMAL: [[f32; 4]; 12] = [
    [0.0, 0.0, 7.0, 3.0],
    [7.0, 0.0, 24.0, 2.0],
    [31.0, 0.0, 9.0, 4.0],
    [37.0, 4.0, 3.0, 5.0],
    [38.0, 9.0, 2.0, 15.0],
    [36.0, 24.0, 4.0, 3.0],
    [34.0, 27.0, 6.0, 5.0],
    [8.0, 29.0, 26.0, 3.0],
    [0.0, 28.0, 8.0, 4.0],
    [0.0, 3.0, 4.0, 6.0],
    [0.0, 9.0, 3.0, 17.0],
    [0.0, 26.0, 5.0, 2.0],
];

fn from_json(value: &Value) -> Rect {
    let [left, top, right, bottom]: [f32; 4] = serde_json::from_value(value.clone()).unwrap();
    assert!(left <= right && top <= bottom);
    Rect::new(left, top, right, bottom)
}

fn png_rect(value: &Value) -> Rect {
    // Bevy UI global bounds already start at the target's top-left corner.
    from_json(value)
}

fn scaled([x, y, width, height]: [f32; 4], scale: f32) -> Rect {
    Rect::new(
        (x * scale).round(),
        (y * scale).round(),
        ((x + width) * scale).round(),
        ((y + height) * scale).round(),
    )
}

fn root(name: &str) -> [f32; 4] {
    match name {
        "normal" => [18.0, 20.0, 40.0, 32.0],
        "minimum" => [96.0, 20.0, 17.0, 18.0],
        "tiny" => [156.0, 20.0, 9.0, 8.0],
        "empty" => [200.0, 20.0, 0.0, 32.0],
        "zero-cap" => [18.0, 88.0, 40.0, 32.0],
        "absent-cap" => [96.0, 88.0, 40.0, 32.0],
        "nested-clipped" => [186.0, 96.0, 40.0, 32.0],
        _ => panic!("unknown generated case {name}"),
    }
}

fn assert_rgb(png: &image::RgbaImage, at: UVec2, expected: [u8; 3]) {
    let actual = png.get_pixel(at.x, at.y).0;
    for (actual, expected) in actual[..3].iter().zip(expected) {
        assert!(
            actual.abs_diff(expected) <= 2,
            "pixel {at:?}: {actual} != {expected}"
        );
    }
}

fn index(source: Rect) -> usize {
    TAGS.iter()
        .enumerate()
        .find(|(index, (_, width, height))| {
            let row = f32::from(u16::try_from(*index).unwrap() * 8 + 1);
            source == Rect::new(2.0, row, 2.0 + f32::from(*width), row + f32::from(*height))
        })
        .unwrap()
        .0
}

#[derive(Default)]
struct Samples {
    checked: usize,
    transparent: usize,
    black: usize,
}

fn check_piece(
    png: &image::RgbaImage,
    destination: Rect,
    source: Rect,
    index: usize,
    samples: &mut Samples,
) {
    for y in destination.min.as_uvec2().y..destination.max.as_uvec2().y {
        for x in destination.min.as_uvec2().x..destination.max.as_uvec2().x {
            let position = (UVec2::new(x, y).as_vec2() + Vec2::splat(0.5) - destination.min)
                / destination.size()
                * source.size();
            let fraction = position.fract();
            // Avoid only samples on a nearest-texel boundary; tiny platform
            // interpolation differences must not choose the adjacent texel.
            if fraction.min_element() < 0.01 || fraction.max_element() > 0.99 {
                continue;
            }
            let texel = position.floor().as_uvec2();
            let expected = if index == 0 && texel.x == 0 {
                samples.transparent += 1;
                BACKGROUND
            } else if index == 0 && texel == UVec2::ONE {
                samples.black += 1;
                [0, 0, 0]
            } else {
                [u8::try_from(index * 17 + 1).unwrap(), 40, 90]
            };
            assert_rgb(png, UVec2::new(x, y), expected);
            samples.checked += 1;
        }
    }
}

pub(super) fn verify(path: &Path, scale: f32) -> Value {
    let png = image::open(path).unwrap().into_rgba8();
    assert_eq!(png.dimensions(), (640, 480));
    let state: Value =
        serde_json::from_slice(&std::fs::read(path.with_extension("layout.json")).unwrap())
            .unwrap();
    let roots = state["roots"].as_array().unwrap();
    assert_eq!(roots.len(), 7);
    for record in roots {
        let name = record["case"].as_str().unwrap();
        assert_eq!(
            png_rect(&record["bounds"]),
            scaled(root(name), scale),
            "{name}"
        );
    }
    let mut samples = Samples::default();
    let mut normal = [false; 12];
    let mut altered_left_edges = [false; 2];
    for record in state["pictures"].as_array().unwrap() {
        let name = record["case"].as_str().unwrap();
        assert_ne!(name, "empty");
        let clip = if name == "nested-clipped" {
            scaled([186.0, 96.0, 22.0, 28.0], scale)
        } else {
            scaled(root(name), scale)
        };
        assert_eq!(png_rect(&record["clip"]), clip, "{name} clip");
        let destination = png_rect(&record["bounds"]);
        assert!(destination.width() > 0.0 && destination.height() > 0.0);
        assert_eq!(destination.min, destination.min.round());
        assert_eq!(destination.max, destination.max.round());
        let source = from_json(&record["source_rect"]);
        let piece_index = index(source);
        if piece_index == 10 && matches!(name, "zero-cap" | "absent-cap") {
            let (slot, top) = if name == "zero-cap" {
                (0, 91.0)
            } else {
                (1, 88.0)
            };
            assert_eq!(destination.min.y, (top * scale).round());
            altered_left_edges[slot] = true;
        }
        if name == "normal" {
            let index = piece_index;
            assert!(!normal[index], "duplicate normal perimeter piece");
            normal[index] = true;
            let [x, y, width, height] = NORMAL[index];
            assert_eq!(
                destination,
                scaled([18.0 + x, 20.0 + y, width, height], scale)
            );
            check_piece(&png, destination, source, index, &mut samples);
        }
    }
    assert!(normal.into_iter().all(|present| present));
    assert!(altered_left_edges.into_iter().all(|present| present));
    assert!(samples.checked > 0 && samples.transparent > 0 && samples.black > 0);
    // Fixed interior colors prove that nested, minimum, tiny and altered-cap
    // cases reached the GPU too; background-only clipping probes cannot do so.
    for (position, expected) in [
        (Vec2::new(187.5, 101.5), [154, 40, 90]),
        (Vec2::new(101.5, 21.5), [1, 40, 90]),
        (Vec2::new(157.5, 21.5), [35, 40, 90]),
        (Vec2::new(19.5, 93.5), [171, 40, 90]),
        (Vec2::new(97.5, 93.5), [171, 40, 90]),
    ] {
        assert_rgb(&png, (position * scale).floor().as_uvec2(), expected);
    }
    // These points would lie on the nested frame's right/bottom art or on
    // the tiny frame's cap without the production ancestor/outer clipping.
    for position in [
        Vec2::new(225.0, 110.0),
        Vec2::new(204.0, 127.0),
        Vec2::new(164.5, 28.5),
    ] {
        assert_rgb(&png, (position * scale).floor().as_uvec2(), BACKGROUND);
    }
    serde_json::json!({
        "state": "PASS", "scale": scale,
        "normal_pieces": 12, "normal_pixel_samples": samples.checked,
        "transparent_margin_samples": samples.transparent,
        "opaque_black_samples": samples.black, "outside_clip_probes": 3,
        "other_case_interior_probes": 5,
        "scope": "synthetic production GPU screenshot/readback, not native or live parity",
    })
}
