//! Item artwork is read from the user's installation, never bundled.
use bevy::prelude::*;
use std::{collections::BTreeMap, path::Path};

#[derive(Resource, Default)]
pub(crate) struct Icons(BTreeMap<u32, Option<Handle<Image>>>);

/// Titanium's 40px drag-item cells are column-major, starting at icon 500.
fn address(icon: u32) -> Option<(u32, Rect)> {
    let index = icon.checked_sub(500)?;
    let sheet = index / 36 + 1;
    let cell = index % 36;
    let x = f32::from(u16::try_from(cell / 6 * 40).ok()?);
    let y = f32::from(u16::try_from(cell % 6 * 40).ok()?);
    Some((sheet, Rect::new(x, y, x + 40.0, y + 40.0)))
}
impl Icons {
    pub(crate) fn get(
        &mut self,
        icon: u32,
        directory: Option<&Path>,
        images: &mut Assets<Image>,
    ) -> Option<ImageNode> {
        let (sheet, rect) = address(icon)?;
        let directory = directory?;
        let handle = self
            .0
            .entry(sheet)
            .or_insert_with(|| {
                let path = directory
                    .join("uifiles/default")
                    .join(format!("dragitem{sheet}.tga"));
                let pixels = image::open(path).ok()?.into_rgba8();
                if pixels.width() != 256 || pixels.height() != 256 {
                    return None;
                }
                let mut texture = Image::new(
                    bevy::render::render_resource::Extent3d {
                        width: 256,
                        height: 256,
                        depth_or_array_layers: 1,
                    },
                    bevy::render::render_resource::TextureDimension::D2,
                    pixels.into_raw(),
                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                    bevy::asset::RenderAssetUsages::RENDER_WORLD,
                );
                texture.sampler = bevy::image::ImageSampler::nearest();
                Some(images.add(texture))
            })
            .as_ref()?;
        Some(ImageNode {
            image: handle.clone(),
            rect: Some(rect),
            ..default()
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icon_cells_follow_columns_and_advance_sheets() {
        assert!(address(499).is_none());
        assert_eq!(address(500), Some((1, Rect::new(0.0, 0.0, 40.0, 40.0))));
        assert_eq!(address(501), Some((1, Rect::new(0.0, 40.0, 40.0, 80.0))));
        assert_eq!(address(506), Some((1, Rect::new(40.0, 0.0, 80.0, 40.0))));
        assert_eq!(address(536), Some((2, Rect::new(0.0, 0.0, 40.0, 40.0))));
    }
}
