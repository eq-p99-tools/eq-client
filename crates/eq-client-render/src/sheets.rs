//! Icons from the installed client's texture sheets, in the UI skin the
//! windows follow (the default skin's sheet where the skin has none of its
//! own), never bundled. Each sheet is read once per skin.
use bevy::prelude::*;
use eq_client_assets::ui::{SHEET_SIZE, texture_sheet};
use std::collections::BTreeMap;

/// Sheets read so far, by skin and file. A sheet that could not be read is
/// remembered as missing, so it is not read again every frame.
#[derive(Resource, Default)]
pub(crate) struct Sheets(BTreeMap<(String, String), Option<Handle<Image>>>);

/// Cuts icons out of the skin's sheets.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Art<'w> {
    sheets: ResMut<'w, Sheets>,
    images: ResMut<'w, Assets<Image>>,
    skin: Res<'w, super::skin::UiSkin>,
    settings: Res<'w, super::ViewerSettings>,
}

impl Art<'_> {
    /// An item's icon.
    pub(crate) fn item(&mut self, icon: u32) -> Option<ImageNode> {
        let (sheet, rect) = item_cell(icon)?;
        self.piece(&format!("dragitem{sheet}.tga"), rect)
    }

    /// A spell's icon.
    pub(crate) fn spell(&mut self, icon: u32) -> Option<ImageNode> {
        let (sheet, rect) = spell_cell(icon);
        self.piece(&format!("spells{sheet:02}.tga"), rect)
    }

    fn piece(&mut self, file: &str, rect: Rect) -> Option<ImageNode> {
        let directory = self.settings.0.eq_directory.as_deref()?;
        let skin = &self.skin.0;
        let images = &mut self.images;
        let handle = self
            .sheets
            .0
            .entry((skin.clone(), file.to_owned()))
            .or_insert_with(|| match texture_sheet(directory, skin, file) {
                Ok(pixels) => Some(images.add(texture(pixels))),
                Err(error) => {
                    warn!("UI sheet unavailable: {error}");
                    None
                }
            })
            .clone()?;
        Some(ImageNode {
            image: handle,
            rect: Some(rect),
            ..default()
        })
    }
}

fn texture(pixels: image::RgbaImage) -> Image {
    let mut texture = Image::new(
        bevy::render::render_resource::Extent3d {
            width: SHEET_SIZE,
            height: SHEET_SIZE,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        pixels.into_raw(),
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    texture.sampler = bevy::image::ImageSampler::nearest();
    texture
}

/// Titanium's 40px drag-item cells are column-major, starting at icon 500.
fn item_cell(icon: u32) -> Option<(u32, Rect)> {
    let index = icon.checked_sub(500)?;
    let sheet = index / 36 + 1;
    let cell = index % 36;
    let x = f32::from(u16::try_from(cell / 6 * 40).ok()?);
    let y = f32::from(u16::try_from(cell % 6 * 40).ok()?);
    Some((sheet, Rect::new(x, y, x + 40.0, y + 40.0)))
}

/// `EQUI_Animations` `A_SpellIcons` uses a horizontal 6-by-6 grid of 40px cells.
fn spell_cell(icon: u32) -> (u32, Rect) {
    let cell = icon % 36;
    let x = f32::from(u16::try_from(cell % 6 * 40).expect("cell is bounded"));
    let y = f32::from(u16::try_from(cell / 6 * 40).expect("cell is bounded"));
    (icon / 36 + 1, Rect::new(x, y, x + 40.0, y + 40.0))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Stands in for the skin's sheets: every sheet reads as one blank image.
    pub(crate) fn blank(app: &mut App) {
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::default());
        let mut sheets = Sheets::default();
        let skin = super::super::skin::UiSkin::default().0;
        for file in (1..=4)
            .map(|sheet| format!("dragitem{sheet}.tga"))
            .chain((1..=4).map(|sheet| format!("spells{sheet:02}.tga")))
        {
            sheets.0.insert((skin.clone(), file), Some(handle.clone()));
        }
        app.insert_resource(sheets)
            .init_resource::<super::super::skin::UiSkin>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_cells_follow_columns_and_advance_sheets() {
        assert!(item_cell(499).is_none());
        assert_eq!(item_cell(500), Some((1, Rect::new(0.0, 0.0, 40.0, 40.0))));
        assert_eq!(item_cell(501), Some((1, Rect::new(0.0, 40.0, 40.0, 80.0))));
        assert_eq!(item_cell(506), Some((1, Rect::new(40.0, 0.0, 80.0, 40.0))));
        assert_eq!(item_cell(536), Some((2, Rect::new(0.0, 0.0, 40.0, 40.0))));
    }

    #[test]
    fn spell_cells_follow_rows_then_sheets() {
        assert_eq!(spell_cell(0), (1, Rect::new(0.0, 0.0, 40.0, 40.0)));
        assert_eq!(spell_cell(1), (1, Rect::new(40.0, 0.0, 80.0, 40.0)));
        assert_eq!(spell_cell(6), (1, Rect::new(0.0, 40.0, 40.0, 80.0)));
        assert_eq!(spell_cell(36), (2, Rect::new(0.0, 0.0, 40.0, 40.0)));
    }
}
