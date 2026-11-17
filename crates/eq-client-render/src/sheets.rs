//! Pictures from the installed client's textures, in the UI skin the
//! windows follow (the default skin's file where the skin has none of its
//! own), never bundled: icons, and the pieces skinned windows are drawn
//! with. Each texture is read once per skin.
use bevy::prelude::*;
use eq_client_assets::{
    sidl::{Library, Piece},
    ui::texture,
};
use std::collections::{BTreeMap, BTreeSet};

type CutKey = (String, String, [u32; 4]);

/// Sheets read so far, by skin and file. A sheet that could not be read is
/// remembered as missing, so it is not read again every frame.
#[derive(Resource, Default)]
pub(crate) struct Sheets {
    read: BTreeMap<(String, String), Option<Handle<Image>>>,
    /// The skin a window is being drawn from, where it is not the player's:
    /// the default skin, for a window the player's skin hides.
    drawing: Option<String>,
    /// Whether an optional icon's region contains any visible pixels.
    visible: BTreeMap<CutKey, bool>,
    /// Animation definitions, including missing libraries, read once per skin.
    libraries: BTreeMap<String, Option<Library>>,
    missing_item_grid: BTreeSet<String>,
}

/// Cuts icons out of the skin's sheets.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Art<'w> {
    sheets: ResMut<'w, Sheets>,
    images: ResMut<'w, Assets<Image>>,
    skin: Res<'w, super::skin::UiSkin>,
    settings: Res<'w, super::ViewerSettings>,
}

impl Art<'_> {
    /// An optional icon, falling back when an installation retains its XML but
    /// blanks the texture region. Cache that check without retaining CPU sheets.
    pub(crate) fn visible_cut(&mut self, piece: &Piece) -> Option<ImageNode> {
        let skin = self.sheets.drawing.as_ref().unwrap_or(&self.skin.0).clone();
        let key = (
            skin.clone(),
            piece.texture.clone(),
            [piece.x, piece.y, piece.width, piece.height],
        );
        let directory = self.settings.0.eq_directory.as_deref()?;
        let visible = *self.sheets.visible.entry(key).or_insert_with(|| {
            texture(directory, &skin, &piece.texture)
                .is_ok_and(|pixels| visible_pixels(&pixels, piece))
        });
        visible.then(|| self.cut(piece)).flatten()
    }

    /// An item's icon in the installed skin's authored drag-item grid. The
    /// existing icon-ID mapping starts at 500; sheet layout comes from SIDL.
    pub(crate) fn item(&mut self, icon: u32) -> Option<ImageNode> {
        let index = icon.checked_sub(500)?;
        let grid = self
            .library()
            .and_then(|library| library.named_grid("A_DragItem"));
        let Some(grid) = grid else {
            let skin = self.sheets.drawing.as_ref().unwrap_or(&self.skin.0).clone();
            if self.sheets.missing_item_grid.insert(skin) {
                warn!(
                    "Item icon layout unavailable; keeping fallback artwork (skin layout unverified)"
                );
            }
            return None;
        };
        let cell = grid.cell(index)?;
        let handle = self.texture(&cell.texture)?;
        let size = self.images.get(&handle)?.size();
        if cell.x.checked_add(cell.width)? > size.x || cell.y.checked_add(cell.height)? > size.y {
            return None;
        }
        self.cut(&cell)
    }

    /// A spell's icon.
    pub(crate) fn spell(&mut self, icon: u32) -> Option<ImageNode> {
        let cell = self.library()?.named_grid("A_SpellIcons")?.cell(icon)?;
        self.cut(&cell)
    }

    /// A picture the official program resolves by animation name, such as
    /// the spellbook's empty-slot frame.
    pub(crate) fn named(&mut self, name: &str) -> Option<ImageNode> {
        let piece = self.library()?.named_piece(name)?;
        self.cut(&piece)
    }

    /// Whether artwork needs to be resolved again for a newly selected skin.
    pub(crate) fn skin_changed(&self) -> bool {
        self.skin.is_changed() || self.settings.is_changed()
    }

    fn library(&mut self) -> Option<&Library> {
        let directory = self.settings.0.eq_directory.as_deref()?;
        let skin = self.sheets.drawing.as_ref().unwrap_or(&self.skin.0).clone();
        self.sheets
            .libraries
            .entry(skin.clone())
            .or_insert_with(|| {
                Library::read(directory, &skin)
                    .inspect_err(|error| warn!("UI icon definitions unavailable: {error}"))
                    .ok()
            })
            .as_ref()
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

    /// Draws from this skin's textures until told otherwise, rather than the
    /// player's skin's: the default skin's, for a window drawn as the default
    /// skin draws it.
    pub(crate) fn draw_in(&mut self, skin: Option<String>) {
        self.sheets.drawing = skin;
    }

    /// The override used by a window when its widgets retain artwork for later.
    pub(crate) fn drawing_skin(&self) -> Option<String> {
        self.sheets.drawing.clone()
    }

    /// One of the skin's textures, whole, such as a window's background.
    pub(crate) fn texture(&mut self, file: &str) -> Option<Handle<Image>> {
        let directory = self.settings.0.eq_directory.as_deref()?;
        let skin = self.sheets.drawing.as_ref().unwrap_or(&self.skin.0).clone();
        let images = &mut self.images;
        self.sheets
            .read
            .entry((skin.clone(), file.to_owned()))
            .or_insert_with(|| match texture(directory, &skin, file) {
                Ok(pixels) => Some(images.add(image(pixels))),
                Err(error) => {
                    warn!("UI texture unavailable: {error}");
                    None
                }
            })
            .clone()
    }

    /// A rectangle of a texture, stretched over its node as the skin's
    /// pieces are: a border's edge spans its whole length.
    fn piece(&mut self, file: &str, rect: Rect) -> Option<ImageNode> {
        Some(ImageNode {
            image: self.texture(file)?,
            rect: Some(rect),
            image_mode: NodeImageMode::Stretch,
            ..default()
        })
    }
}

/// Transparent or out-of-bounds artwork must not hide a functional fallback.
fn visible_pixels(pixels: &image::RgbaImage, piece: &Piece) -> bool {
    let Some(right) = piece
        .x
        .checked_add(piece.width)
        .filter(|right| *right <= pixels.width())
    else {
        return false;
    };
    let Some(bottom) = piece
        .y
        .checked_add(piece.height)
        .filter(|bottom| *bottom <= pixels.height())
    else {
        return false;
    };
    (piece.y..bottom).any(|y| (piece.x..right).any(|x| pixels.get_pixel(x, y)[3] != 0))
}

pub(crate) fn image(pixels: image::RgbaImage) -> Image {
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

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Supplies generated artwork to the ordinary skin rectangle/upload path.
    pub(crate) fn sheet(app: &mut App, file: &str, pixels: image::RgbaImage) -> Handle<Image> {
        let skin = app
            .world()
            .resource::<super::super::skin::UiSkin>()
            .0
            .clone();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(image(pixels));
        app.world_mut()
            .resource_mut::<Sheets>()
            .read
            .insert((skin, file.to_owned()), Some(handle.clone()));
        handle
    }

    /// Stands in for the skin's sheets: every sheet reads as one blank image.
    pub(crate) fn blank(app: &mut App) {
        use std::fmt::Write as _;
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(image(image::RgbaImage::new(256, 256)));
        let mut sheets = Sheets::default();
        let skin = app
            .world()
            .get_resource::<crate::skin::UiSkin>()
            .map_or_else(|| crate::skin::UiSkin::default().0, |skin| skin.0.clone());
        for file in (1..=4)
            .map(|sheet| format!("dragitem{sheet}.tga"))
            .chain((1..=4).map(|sheet| format!("spells{sheet:02}.tga")))
        {
            sheets
                .read
                .insert((skin.clone(), file), Some(handle.clone()));
        }
        let mut frames = String::new();
        let mut items = String::new();
        for sheet in 1..=4 {
            write!(frames, "<Frames><Texture>spells{sheet:02}.tga</Texture><Location><X>0</X><Y>0</Y></Location>\
                <Size><CX>240</CX><CY>240</CY></Size></Frames>").unwrap();
            write!(items, "<Frames><Texture>dragitem{sheet}.tga</Texture><Size><CX>256</CX><CY>256</CY></Size></Frames>").unwrap();
        }
        let animations = format!(
            "<XML><Ui2DAnimation item=\"A_SpellIcons\">\
            <Grid>true</Grid><CellWidth>40</CellWidth><CellHeight>40</CellHeight>\
            {frames}</Ui2DAnimation><Ui2DAnimation item=\"A_DragItem\">\
            <Grid>true</Grid><Vertical>true</Vertical><CellWidth>40</CellWidth><CellHeight>40</CellHeight>\
            {items}</Ui2DAnimation></XML>"
        );
        sheets
            .libraries
            .insert(skin, Some(Library::parse(&animations, "<XML/>").unwrap()));
        app.insert_resource(sheets)
            .init_resource::<super::super::skin::UiSkin>();
    }

    /// Supplies synthetic SIDL while retaining the ordinary sheet/image path.
    pub(crate) fn definitions(app: &mut App, animations: &str) {
        let skin = app.world().resource::<crate::skin::UiSkin>().0.clone();
        app.world_mut()
            .resource_mut::<Sheets>()
            .libraries
            .insert(skin, Some(Library::parse(animations, "<XML/>").unwrap()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn item_registration_uses_authored_origins_cells_order_and_real_file_dimensions() {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("synthetic-install".into());
        let first = testing::sheet(&mut app, "custom-b.tga", image::RgbaImage::new(40, 30));
        let second = testing::sheet(&mut app, "custom-a.tga", image::RgbaImage::new(20, 15));
        testing::definitions(
            &mut app,
            "<XML><Ui2DAnimation item=\"A_DragItem\">\
            <Grid>true</Grid><Vertical>true</Vertical><CellWidth>14</CellWidth><CellHeight>10</CellHeight>\
            <Frames><Texture>custom-b.tga</Texture><Location><X>3</X><Y>5</Y></Location>\
            <Size><CX>28</CX><CY>20</CY></Size></Frames>\
            <Frames><Texture>custom-a.tga</Texture><Location><X>2</X><Y>3</Y></Location>\
            <Size><CX>14</CX><CY>10</CY></Size></Frames></Ui2DAnimation></XML>",
        );
        app.world_mut()
            .run_system_once(move |mut art: Art| {
                assert!(art.item(499).is_none());
                for (id, handle, rect) in [
                    (500, first.clone(), Rect::new(3.0, 5.0, 17.0, 15.0)),
                    (501, first.clone(), Rect::new(3.0, 15.0, 17.0, 25.0)),
                    (502, first.clone(), Rect::new(17.0, 5.0, 31.0, 15.0)),
                    (503, first.clone(), Rect::new(17.0, 15.0, 31.0, 25.0)),
                    (504, second.clone(), Rect::new(2.0, 3.0, 16.0, 13.0)),
                ] {
                    let image = art.item(id).unwrap();
                    assert_eq!(image.image, handle);
                    assert_eq!(image.rect, Some(rect));
                }
                assert!(art.item(505).is_none());
            })
            .unwrap();
    }

    #[test]
    fn item_metadata_missing_or_outside_the_actual_image_preserves_fallback() {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("synthetic-install".into());
        testing::sheet(&mut app, "custom.tga", image::RgbaImage::new(12, 10));
        testing::definitions(
            &mut app,
            "<XML><Ui2DAnimation item=\"A_DragItem\">\
            <Grid>true</Grid><CellWidth>12</CellWidth><CellHeight>10</CellHeight>\
            <Frames><Texture>custom.tga</Texture><Size><CX>24</CX><CY>10</CY></Size></Frames>\
            </Ui2DAnimation></XML>",
        );
        app.world_mut()
            .run_system_once(|mut art: Art| {
                assert!(art.item(500).is_some());
                assert!(art.item(501).is_none());
            })
            .unwrap();
        testing::definitions(&mut app, "<XML/>");
        app.world_mut()
            .run_system_once(|mut art: Art| {
                assert!(art.item(500).is_none());
            })
            .unwrap();
    }

    #[test]
    fn item_grids_resolve_per_selected_skin_and_support_horizontal_numbering() {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("synthetic-install".into());
        let skin = app.world().resource::<crate::skin::UiSkin>().0.clone();
        testing::sheet(&mut app, "cells.tga", image::RgbaImage::new(24, 20));
        testing::definitions(
            &mut app,
            "<XML><Ui2DAnimation item=\"A_DragItem\">\
            <Grid>true</Grid><CellWidth>12</CellWidth><CellHeight>10</CellHeight>\
            <Frames><Texture>cells.tga</Texture><Size><CX>24</CX><CY>20</CY></Size></Frames>\
            </Ui2DAnimation></XML>",
        );
        app.world_mut().resource_mut::<crate::skin::UiSkin>().0 = "other".into();
        testing::sheet(&mut app, "other.tga", image::RgbaImage::new(8, 8));
        testing::definitions(
            &mut app,
            "<XML><Ui2DAnimation item=\"A_DragItem\">\
            <Grid>true</Grid><CellWidth>8</CellWidth><CellHeight>8</CellHeight>\
            <Frames><Texture>other.tga</Texture><Size><CX>8</CX><CY>8</CY></Size></Frames>\
            </Ui2DAnimation></XML>",
        );
        app.world_mut()
            .run_system_once(move |mut art: Art| {
                assert_eq!(
                    art.item(500).unwrap().rect,
                    Some(Rect::new(0.0, 0.0, 8.0, 8.0))
                );
                assert!(art.item(501).is_none());
                art.draw_in(Some(skin.clone()));
                assert_eq!(
                    art.item(501).unwrap().rect,
                    Some(Rect::new(12.0, 0.0, 24.0, 10.0))
                );
                assert_eq!(
                    art.item(502).unwrap().rect,
                    Some(Rect::new(0.0, 10.0, 12.0, 20.0))
                );
                art.draw_in(None);
                assert!(art.item(501).is_none());
            })
            .unwrap();
    }

    #[test]
    fn empty_icon_regions_preserve_the_fallback_even_on_nonempty_sheets() {
        let mut pixels = image::RgbaImage::new(4, 4);
        pixels.put_pixel(0, 0, image::Rgba([255; 4]));
        let mut piece = Piece {
            texture: "synthetic.tga".into(),
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        };
        assert!(!visible_pixels(&pixels, &piece));
        pixels.put_pixel(2, 2, image::Rgba([0, 0, 0, 128]));
        assert!(visible_pixels(&pixels, &piece));
        piece.x = u32::MAX;
        assert!(!visible_pixels(&pixels, &piece));
        piece.x = 3;
        assert!(!visible_pixels(&pixels, &piece));
    }

    #[test]
    fn spell_icons_follow_the_skins_declared_grid() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>();
        testing::blank(&mut app);
        app.insert_resource(crate::ViewerSettings(crate::ViewerConfig {
            eq_directory: Some("synthetic-install".into()),
            ..default()
        }));
        let skin = crate::skin::UiSkin::default().0;
        let mut sheets = app.world_mut().resource_mut::<Sheets>();
        let library = Library::parse("<XML><Ui2DAnimation item=\"A_SpellIcons\">\
            <Grid>true</Grid><Vertical>true</Vertical><CellWidth>12</CellWidth><CellHeight>10</CellHeight>\
            <Frames><Texture>spells02.tga</Texture><Location><X>3</X><Y>5</Y></Location>\
            <Size><CX>24</CX><CY>20</CY></Size></Frames>\
            <Frames><Texture>spells01.tga</Texture><Location><X>8</X><Y>9</Y></Location>\
            <Size><CX>12</CX><CY>10</CY></Size></Frames>\
            </Ui2DAnimation></XML>", "<XML/>").unwrap();
        sheets.libraries.insert(skin, Some(library));
        app.world_mut()
            .run_system_once(|mut art: Art| {
                assert_eq!(
                    art.spell(1).unwrap().rect,
                    Some(Rect::new(3.0, 15.0, 15.0, 25.0))
                );
                assert_eq!(
                    art.spell(4).unwrap().rect,
                    Some(Rect::new(8.0, 9.0, 20.0, 19.0))
                );
                assert!(art.spell(5).is_none());
            })
            .unwrap();
    }
}
