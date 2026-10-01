//! Pictures from the installed client's textures, in the UI skin the
//! windows follow (the default skin's file where the skin has none of its
//! own), never bundled: icons, and the pieces skinned windows are drawn
//! with. Each texture is read once per skin.
use bevy::prelude::*;
use eq_client_assets::{sidl::Piece, ui::texture};
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

    /// A piece of one of the skin's textures, as a skin window draws it.
    pub(crate) fn cut(&mut self, piece: &Piece) -> Option<ImageNode> {
        let [x, y, width, height] =
            [piece.x, piece.y, piece.width, piece.height].map(|value| u16::try_from(value).ok());
        let (x, y) = (f32::from(x?), f32::from(y?));
        self.piece(
            &piece.texture,
            Rect::new(x, y, x + f32::from(width?), y + f32::from(height?)),
        )
    }

    /// One of the skin's textures, whole, such as a window's background.
    pub(crate) fn texture(&mut self, file: &str) -> Option<Handle<Image>> {
        let directory = self.settings.0.eq_directory.as_deref()?;
        let skin = &self.skin.0;
        let images = &mut self.images;
        self.sheets
            .0
            .entry((skin.clone(), file.to_owned()))
            .or_insert_with(|| match texture(directory, skin, file) {
                Ok(pixels) => Some(images.add(image(pixels))),
                Err(error) => {
                    warn!("UI texture unavailable: {error}");
                    None
                }
            })
            .clone()
    }

    fn piece(&mut self, file: &str, rect: Rect) -> Option<ImageNode> {
        Some(ImageNode {
            image: self.texture(file)?,
            rect: Some(rect),
            ..default()
        })
    }
}

fn image(pixels: image::RgbaImage) -> Image {
    let (width, height) = pixels.dimensions();
    let mut texture = Image::new(
        bevy::render::render_resource::Extent3d {
            width,
            height,
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
