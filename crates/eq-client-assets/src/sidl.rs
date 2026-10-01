//! The installed client's interface definitions (SIDL): the windows a skin
//! defines, what each shows and where, and the texture pieces they are drawn
//! with. Read at runtime from the user's installation, so no skin data is
//! copied into this project and a custom skin carries over.
//!
//! A skin's files define elements once at the top level, by name: pieces of
//! textures (`Ui2DAnimation`), window chrome (`WindowDrawTemplate`), and the
//! gauges, labels and images a window lists as its `Pieces`.
use super::ui::{Area, UiLayoutError, skin_file};
use std::{collections::HashMap, path::Path};

/// A rectangle cut from one of the skin's textures: the first frame of a
/// `Ui2DAnimation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The texture file, in the skin's directory.
    pub texture: String,
    /// Left edge in the texture.
    pub x: u32,
    /// Top edge in the texture.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// A window's frame: the eight border pieces around it, from the top left.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Border {
    /// The top left corner.
    pub top_left: Option<Piece>,
    /// The top edge, stretched between the corners.
    pub top: Option<Piece>,
    /// The top right corner.
    pub top_right: Option<Piece>,
    /// The right edge.
    pub right: Option<Piece>,
    /// The bottom right corner.
    pub bottom_right: Option<Piece>,
    /// The bottom edge.
    pub bottom: Option<Piece>,
    /// The bottom left corner.
    pub bottom_left: Option<Piece>,
    /// The left edge.
    pub left: Option<Piece>,
}

/// How a window is drawn: its tiled background, its border and its title
/// bar's left, middle and right pieces.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowTemplate {
    /// The texture tiled behind the window.
    pub background: Option<String>,
    /// The frame around the window.
    pub border: Border,
    /// The title bar's left end, its middle, stretched, and its right end.
    pub title: [Option<Piece>; 3],
}

/// How a gauge is drawn: its empty bar, its fill, the lines over it and its
/// end caps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GaugeLook {
    /// The empty bar.
    pub background: Option<Piece>,
    /// The bar's filled part, tinted.
    pub fill: Option<Piece>,
    /// The tick lines over the bar.
    pub lines: Option<Piece>,
    /// The bar's left end.
    pub cap_left: Option<Piece>,
    /// The bar's right end.
    pub cap_right: Option<Piece>,
}

/// A bar that shows a fraction, such as the player's hit points, with a
/// line of text above it.
#[derive(Clone, Debug, PartialEq)]
pub struct Gauge {
    /// Where the gauge sits in its window.
    pub area: Area,
    /// What the gauge shows: the official client's numbering, such as 1 for
    /// the player's hit points and 6 for the target's.
    pub eq_type: Option<u32>,
    /// How it is drawn.
    pub look: GaugeLook,
    /// The fill's colour.
    pub fill_tint: Option<[u8; 3]>,
    /// The text's colour.
    pub text_color: Option<[u8; 3]>,
    /// The text's offset from the gauge's top left.
    pub text_offset: (f32, f32),
    /// How far down the gauge the bar sits.
    pub bar_offset: f32,
}

/// How a button is drawn in each of its states.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ButtonLook {
    /// At rest.
    pub normal: Option<Piece>,
    /// Held down, or on.
    pub pressed: Option<Piece>,
    /// Under the pointer.
    pub flyby: Option<Piece>,
    /// Unusable.
    pub disabled: Option<Piece>,
    /// On, and under the pointer.
    pub pressed_flyby: Option<Piece>,
}

/// A button.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    /// What the window calls it (`ScreenID`), such as `CSPW_SpellBook`.
    pub id: Option<String>,
    /// Where it sits in its window.
    pub area: Area,
    /// How it is drawn.
    pub look: ButtonLook,
    /// Whether it stays on once pressed, as a window's toggle does.
    pub checkbox: bool,
    /// Its words, if it has any.
    pub text: Option<String>,
    /// Its words' colour.
    pub text_color: Option<[u8; 3]>,
    /// A picture on the button, such as a coin on a money button.
    pub decal: Option<Piece>,
    /// Where the picture sits, from the button's top left, and its size.
    pub decal_area: Option<Area>,
    /// The tooltip the skin gives it.
    pub tooltip: Option<String>,
}

/// A slot that holds an item: one of the player's, or of a container.
#[derive(Clone, Debug, PartialEq)]
pub struct InvSlot {
    /// What the window calls it, such as `InvSlot1`.
    pub id: Option<String>,
    /// Where it sits in its window.
    pub area: Area,
    /// The slot's number in the official client's numbering (`EQType`): the
    /// Titanium inventory slot for the player's own, the place in the
    /// container for a container's.
    pub slot: Option<u32>,
    /// What the empty slot shows, such as an ear for an ear slot.
    pub background: Option<Piece>,
}

/// One page of a set of tabs.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    /// What the window calls it.
    pub name: String,
    /// The words on its tab.
    pub title: Option<String>,
    /// Where it sits in its window, when the skin places it.
    pub area: Option<Area>,
    /// How its frame is drawn.
    pub template: Option<WindowTemplate>,
    /// What it shows.
    pub pieces: Vec<(String, Element)>,
    /// The picture on its tab, and while it is the page shown.
    pub icon: [Option<Piece>; 2],
    /// What its tab says under the pointer.
    pub tooltip: Option<String>,
}

/// A window inside a window, such as the inventory's character view.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// What the window calls it, such as `IW_CharacterView`.
    pub name: String,
    /// Where it sits in its window.
    pub area: Area,
    /// How its frame is drawn.
    pub template: Option<WindowTemplate>,
    /// Whether it has a border.
    pub border: bool,
    /// The tooltip the skin gives it.
    pub tooltip: Option<String>,
}

/// One of the gems that hold the player's memorized spells.
#[derive(Clone, Debug, PartialEq)]
pub struct SpellGem {
    /// What the window calls it, such as `CSPW_Spell0` for the first gem.
    pub id: Option<String>,
    /// Where it sits in its window.
    pub area: Area,
    /// The frame around the gem's icon.
    pub holder: Option<Piece>,
    /// Behind the icon.
    pub background: Option<Piece>,
    /// Over the gem while it is under the pointer.
    pub highlight: Option<Piece>,
}

/// Where a label's text sits in its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// At the left.
    #[default]
    Left,
    /// In the middle.
    Center,
    /// At the right.
    Right,
}

/// A line of text.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// Where the label sits in its window.
    pub area: Area,
    /// What the label shows, in the official client's numbering; None for
    /// fixed text.
    pub eq_type: Option<u32>,
    /// The text, or a sample of it when the label shows something.
    pub text: String,
    /// The text's colour.
    pub color: Option<[u8; 3]>,
    /// Where the text sits in the label's box.
    pub align: Align,
    /// The official client's font number, from 1 for the smallest.
    pub font: Option<u8>,
}

/// One thing a window shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    /// A bar showing a fraction.
    Gauge(Gauge),
    /// A line of text.
    Label(Label),
    /// A still picture, from a `StaticAnimation`.
    Image {
        /// What the window calls it (`ScreenID`), such as
        /// `A_AttackIndicatorAnim`, which shows only while attacking.
        id: Option<String>,
        /// Where it sits in its window.
        area: Area,
        /// The picture.
        piece: Piece,
    },
    /// A button.
    Button(Button),
    /// A spell gem.
    SpellGem(SpellGem),
    /// An item slot.
    InvSlot(InvSlot),
    /// Pages behind tabs; the first is shown.
    Tabs(Vec<Page>),
    /// A window inside the window.
    View(View),
    /// An element this reader does not draw yet, by its kind.
    Other(String),
}

/// A window as the skin defines it.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    /// The window's name, such as `PlayerWindow`.
    pub name: String,
    /// Its default place and size on an 800 by 600 screen, borders and
    /// title bar included.
    pub area: Area,
    /// How its frame is drawn.
    pub template: Option<WindowTemplate>,
    /// Whether it has a title bar.
    pub titlebar: bool,
    /// Whether it has a border.
    pub border: bool,
    /// The tooltip the skin gives it.
    pub tooltip: Option<String>,
    /// What it shows, in drawing order, by element name.
    pub pieces: Vec<(String, Element)>,
}

/// The pieces and window templates a skin's windows refer to by name.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pieces: HashMap<String, Piece>,
    templates: HashMap<String, WindowTemplate>,
}

impl Library {
    /// Reads a skin's `EQUI_Animations.xml` and `EQUI_Templates.xml`, each from
    /// the default skin when the skin has none of its own.
    ///
    /// # Errors
    /// Rejects unsafe skin names and unreadable or malformed files.
    pub fn read(eq_directory: &Path, skin: &str) -> Result<Self, UiLayoutError> {
        let animations = read_text(eq_directory, skin, "EQUI_Animations.xml")?;
        let templates = read_text(eq_directory, skin, "EQUI_Templates.xml")?;
        Self::parse(&animations, &templates)
    }

    /// The library in these two files' texts.
    ///
    /// # Errors
    /// Rejects malformed XML.
    pub fn parse(animations: &str, templates: &str) -> Result<Self, UiLayoutError> {
        let mut library = Self::default();
        library.add_pieces(&roxmltree::Document::parse(animations)?);
        let document = roxmltree::Document::parse(templates)?;
        library.add_pieces(&document);
        for node in document.root_element().children() {
            if node.has_tag_name("WindowDrawTemplate")
                && let Some(name) = node.attribute("item")
            {
                let template = library.template(node);
                library.templates.insert(name.to_owned(), template);
            }
        }
        Ok(library)
    }

    /// Reads a window from one of the skin's window files, such as
    /// `PlayerWindow` from `EQUI_PlayerWindow.xml`.
    ///
    /// # Errors
    /// Rejects unsafe names, unreadable or malformed files, and files that
    /// define no such window.
    pub fn window(
        &self,
        eq_directory: &Path,
        skin: &str,
        file: &str,
        name: &str,
    ) -> Result<Screen, UiLayoutError> {
        self.screen(&read_text(eq_directory, skin, file)?, name)
    }

    /// The window of this name in a window file's text. The file's own
    /// pieces come first, then the library's.
    ///
    /// # Errors
    /// Rejects malformed XML and files that define no such window.
    pub fn screen(&self, text: &str, name: &str) -> Result<Screen, UiLayoutError> {
        let document = roxmltree::Document::parse(text)?;
        let mut local = self.clone();
        local.add_pieces(&document);
        let elements: HashMap<&str, roxmltree::Node<'_, '_>> = document
            .root_element()
            .children()
            .filter_map(|node| Some((node.attribute("item")?, node)))
            .collect();
        let screen = elements
            .get(name)
            .filter(|node| node.has_tag_name("Screen"))
            .ok_or_else(|| UiLayoutError::MissingWindow(name.to_owned()))?;
        let pieces = local.pieces(*screen, &elements, 0);
        Ok(Screen {
            name: name.to_owned(),
            area: area(*screen).unwrap_or(Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
            template: text_of(*screen, "DrawTemplate")
                .and_then(|template| local.templates.get(template).cloned()),
            titlebar: flag(*screen, "Style_Titlebar"),
            border: flag(*screen, "Style_Border"),
            tooltip: text_of(*screen, "TooltipReference").map(str::to_owned),
            pieces,
        })
    }

    /// Notes every `Ui2DAnimation` in a document by name, as its first frame.
    fn add_pieces(&mut self, document: &roxmltree::Document<'_>) {
        for node in document.root_element().children() {
            if node.has_tag_name("Ui2DAnimation")
                && let Some(name) = node.attribute("item")
                && let Some(piece) = child(node, "Frames").and_then(frame)
            {
                self.pieces.insert(name.to_owned(), piece);
            }
        }
    }

    fn piece(&self, name: Option<&str>) -> Option<Piece> {
        self.pieces.get(name?).cloned()
    }

    fn template(&self, node: roxmltree::Node<'_, '_>) -> WindowTemplate {
        let border = child(node, "Border");
        let side = |name: &str| self.piece(border.and_then(|border| text_of(border, name)));
        let title = child(node, "Titlebar");
        let title_piece = |name: &str| self.piece(title.and_then(|title| text_of(title, name)));
        WindowTemplate {
            background: text_of(node, "Background").map(str::to_owned),
            border: Border {
                top_left: side("TopLeft"),
                top: side("Top"),
                top_right: side("TopRight"),
                right: side("Right"),
                bottom_right: side("BottomRight"),
                bottom: side("Bottom"),
                bottom_left: side("BottomLeft"),
                left: side("Left"),
            },
            title: [
                title_piece("Left"),
                title_piece("Middle"),
                title_piece("Right"),
            ],
        }
    }

    /// What a screen or page lists as its pieces, in drawing order. A piece
    /// may name its kind first, as `Page:IW_InvPage` does.
    fn pieces(
        &self,
        node: roxmltree::Node<'_, '_>,
        elements: &HashMap<&str, roxmltree::Node<'_, '_>>,
        depth: u8,
    ) -> Vec<(String, Element)> {
        node.children()
            .filter(|child| child.has_tag_name("Pieces"))
            .filter_map(|child| child.text())
            .map(str::trim)
            .filter_map(|piece| {
                let name = piece.split_once(':').map_or(piece, |(_, name)| name.trim());
                let element = elements.get(name)?;
                Some((name.to_owned(), self.element(*element, elements, depth)))
            })
            .collect()
    }

    fn button(&self, node: roxmltree::Node<'_, '_>) -> Element {
        let at = || {
            area(node).unwrap_or(Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            })
        };
        let look = child(node, "ButtonDrawTemplate");
        let state = |name: &str| self.piece(look.and_then(|look| text_of(look, name)));
        Element::Button(Button {
            id: text_of(node, "ScreenID").map(str::to_owned),
            area: at(),
            look: ButtonLook {
                normal: state("Normal"),
                pressed: state("Pressed"),
                flyby: state("Flyby"),
                disabled: state("Disabled"),
                pressed_flyby: state("PressedFlyby"),
            },
            checkbox: flag(node, "Style_Checkbox"),
            text: text_of(node, "Text").map(str::to_owned),
            text_color: color(node, "TextColor"),
            decal: state("NormalDecal"),
            decal_area: child(node, "DecalSize").and_then(|size| {
                let offset = child(node, "DecalOffset");
                Some(Area {
                    x: offset.and_then(|at| number(at, "X")).unwrap_or(0.0),
                    y: offset.and_then(|at| number(at, "Y")).unwrap_or(0.0),
                    width: number(size, "CX")?,
                    height: number(size, "CY")?,
                })
            }),
            tooltip: text_of(node, "TooltipReference").map(str::to_owned),
        })
    }

    fn pages(
        &self,
        node: roxmltree::Node<'_, '_>,
        elements: &HashMap<&str, roxmltree::Node<'_, '_>>,
        depth: u8,
    ) -> Vec<Page> {
        node.children()
            .filter(|child| child.has_tag_name("Pages"))
            .filter_map(|child| child.text())
            .map(str::trim)
            .filter_map(|page| {
                let name = page.split_once(':').map_or(page, |(_, name)| name.trim());
                let page = elements.get(name)?;
                Some(Page {
                    name: name.to_owned(),
                    title: text_of(*page, "TabText").map(str::to_owned),
                    area: area(*page),
                    template: text_of(*page, "DrawTemplate")
                        .and_then(|template| self.templates.get(template).cloned()),
                    pieces: self.pieces(*page, elements, depth + 1),
                    icon: [
                        self.piece(text_of(*page, "TabIcon")),
                        self.piece(text_of(*page, "TabIconActive")),
                    ],
                    tooltip: text_of(*page, "TooltipReference").map(str::to_owned),
                })
            })
            .collect()
    }

    fn element(
        &self,
        node: roxmltree::Node<'_, '_>,
        elements: &HashMap<&str, roxmltree::Node<'_, '_>>,
        depth: u8,
    ) -> Element {
        let at = || {
            area(node).unwrap_or(Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            })
        };
        match node.tag_name().name() {
            "Gauge" => {
                let look = child(node, "GaugeDrawTemplate");
                let part = |name: &str| self.piece(look.and_then(|look| text_of(look, name)));
                Element::Gauge(Gauge {
                    area: at(),
                    eq_type: number(node, "EQType"),
                    look: GaugeLook {
                        background: part("Background"),
                        fill: part("Fill"),
                        lines: part("Lines"),
                        cap_left: part("EndCapLeft"),
                        cap_right: part("EndCapRight"),
                    },
                    fill_tint: color(node, "FillTint"),
                    text_color: color(node, "TextColor"),
                    text_offset: (
                        number(node, "TextOffsetX").unwrap_or(0.0),
                        number(node, "TextOffsetY").unwrap_or(0.0),
                    ),
                    bar_offset: number(node, "GaugeOffsetY").unwrap_or(0.0),
                })
            }
            "Label" => Element::Label(Label {
                area: at(),
                eq_type: number(node, "EQType"),
                text: text_of(node, "Text").unwrap_or_default().to_owned(),
                color: color(node, "TextColor"),
                align: if flag(node, "AlignCenter") {
                    Align::Center
                } else if flag(node, "AlignRight") {
                    Align::Right
                } else {
                    Align::Left
                },
                font: number(node, "Font"),
            }),
            "Button" => self.button(node),
            "InvSlot" => Element::InvSlot(InvSlot {
                id: text_of(node, "ScreenID").map(str::to_owned),
                area: at(),
                slot: number(node, "EQType"),
                background: self.piece(text_of(node, "Background")),
            }),
            // Pages and windows within windows nest; skins go a few deep.
            "TabBox" if depth < 4 => Element::Tabs(self.pages(node, elements, depth)),
            "Screen" => Element::View(View {
                name: node.attribute("item").unwrap_or_default().to_owned(),
                area: at(),
                template: text_of(node, "DrawTemplate")
                    .and_then(|template| self.templates.get(template).cloned()),
                border: flag(node, "Style_Border"),
                tooltip: text_of(node, "TooltipReference").map(str::to_owned),
            }),
            "SpellGem" => {
                let look = child(node, "SpellGemDrawTemplate");
                let part = |name: &str| self.piece(look.and_then(|look| text_of(look, name)));
                Element::SpellGem(SpellGem {
                    id: text_of(node, "ScreenID").map(str::to_owned),
                    area: at(),
                    holder: part("Holder"),
                    background: part("Background"),
                    highlight: part("Highlight"),
                })
            }
            "StaticAnimation" => match self.piece(text_of(node, "Animation")) {
                Some(piece) => {
                    let mut area = at();
                    // A picture without a size of its own is its piece's size.
                    if area.width <= 0.0 || area.height <= 0.0 {
                        area.width = to_f32(piece.width);
                        area.height = to_f32(piece.height);
                    }
                    Element::Image {
                        id: text_of(node, "ScreenID").map(str::to_owned),
                        area,
                        piece,
                    }
                }
                None => Element::Other("StaticAnimation".into()),
            },
            other => Element::Other(other.to_owned()),
        }
    }
}

/// A window file's text, from the skin or the default skin, decoded leniently:
/// skins declare ASCII, but hand-edited ones may carry Latin-1 text.
fn read_text(eq_directory: &Path, skin: &str, file: &str) -> Result<String, UiLayoutError> {
    let path = skin_file(eq_directory, skin, file)?;
    let bytes = std::fs::read(&path).map_err(|source| UiLayoutError::Read { path, source })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children().find(|child| child.has_tag_name(name))
}

fn text_of<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name)?
        .text()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn number<T: std::str::FromStr>(node: roxmltree::Node<'_, '_>, name: &str) -> Option<T> {
    text_of(node, name)?.parse().ok()
}

fn flag(node: roxmltree::Node<'_, '_>, name: &str) -> bool {
    text_of(node, name).is_some_and(|text| text.eq_ignore_ascii_case("true"))
}

fn color(node: roxmltree::Node<'_, '_>, name: &str) -> Option<[u8; 3]> {
    let color = child(node, name)?;
    Some([
        number(color, "R")?,
        number(color, "G")?,
        number(color, "B")?,
    ])
}

/// An element's place and size, in its container's pixels.
fn area(node: roxmltree::Node<'_, '_>) -> Option<Area> {
    let location = child(node, "Location");
    let size = child(node, "Size")?;
    Some(Area {
        x: location.and_then(|at| number(at, "X")).unwrap_or(0.0),
        y: location.and_then(|at| number(at, "Y")).unwrap_or(0.0),
        width: number(size, "CX")?,
        height: number(size, "CY")?,
    })
}

/// A `Frames` element: a texture and the rectangle cut from it.
fn frame(node: roxmltree::Node<'_, '_>) -> Option<Piece> {
    let location = child(node, "Location");
    let size = child(node, "Size")?;
    Some(Piece {
        texture: text_of(node, "Texture")?.to_owned(),
        x: location.and_then(|at| number(at, "X")).unwrap_or(0),
        y: location.and_then(|at| number(at, "Y")).unwrap_or(0),
        width: number(size, "CX")?,
        height: number(size, "CY")?,
    })
}

#[allow(clippy::cast_precision_loss, reason = "texture sizes are small")]
const fn to_f32(value: u32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANIMATIONS: &str = r#"<XML>
        <Ui2DAnimation item="A_Fill"><Frames><Texture>pieces.tga</Texture>
            <Location><X>8</X><Y>18</Y></Location><Size><CX>100</CX><CY>10</CY></Size></Frames>
        </Ui2DAnimation>
        <Ui2DAnimation item="A_Back"><Frames><Texture>pieces.tga</Texture>
            <Location><X>8</X><Y>7</Y></Location><Size><CX>100</CX><CY>10</CY></Size></Frames>
        </Ui2DAnimation>
        <Ui2DAnimation item="A_Corner"><Frames><Texture>frame.tga</Texture>
            <Size><CX>4</CX><CY>4</CY></Size></Frames>
        </Ui2DAnimation>
    </XML>"#;

    const TEMPLATES: &str = r#"<XML>
        <WindowDrawTemplate item="WDT_Plain">
            <Background>rock.tga</Background>
            <Border><TopLeft>A_Corner</TopLeft><Top>A_Missing</Top></Border>
            <Titlebar><Middle>A_Back</Middle></Titlebar>
        </WindowDrawTemplate>
    </XML>"#;

    const WINDOW: &str = r#"<?xml version="1.0" encoding="us-ascii"?>
    <XML ID="EQInterfaceDefinitionLanguage">
        <TextureInfo item="box.tga"><Size><CX>128</CX><CY>32</CY></Size></TextureInfo>
        <Ui2DAnimation item="A_Box"><Frames><Texture>box.tga</Texture>
            <Size><CX>116</CX><CY>30</CY></Size></Frames></Ui2DAnimation>
        <StaticAnimation item="BoxPicture"><Animation>A_Box</Animation></StaticAnimation>
        <Gauge item="Health">
            <Location><X>5</X><Y>2</Y></Location><Size><CX>108</CX><CY>27</CY></Size>
            <TextOffsetX>8</TextOffsetX><GaugeOffsetY>16</GaugeOffsetY>
            <FillTint><R>240</R><G>0</G><B>0</B></FillTint>
            <EQType>6</EQType>
            <GaugeDrawTemplate><Background>A_Back</Background><Fill>A_Fill</Fill></GaugeDrawTemplate>
        </Gauge>
        <Label item="Percent">
            <EQType>29</EQType>
            <Location><X>7</X><Y>17</Y></Location><Size><CX>60</CX><CY>12</CY></Size>
            <Text>100</Text><AlignRight>true</AlignRight><Font>1</Font>
        </Label>
        <SpellGem item="Gem0"><ScreenID>CSPW_Spell0</ScreenID>
            <Location><X>3</X><Y>10</Y></Location><Size><CX>36</CX><CY>28</CY></Size>
            <SpellGemDrawTemplate><Holder>A_Back</Holder></SpellGemDrawTemplate>
        </SpellGem>
        <Button item="Book"><ScreenID>CSPW_SpellBook</ScreenID>
            <Location><X>10</X><Y>252</Y></Location><Size><CX>22</CX><CY>22</CY></Size>
            <TooltipReference>Opens and closes Your Spellbook</TooltipReference>
            <Style_Checkbox>true</Style_Checkbox>
            <ButtonDrawTemplate><Normal>A_Fill</Normal><Pressed>A_Back</Pressed></ButtonDrawTemplate>
        </Button>
        <InvSlot item="Ear"><ScreenID>InvSlot1</ScreenID>
            <Location><X>123</X><Y>12</Y></Location><Size><CX>42</CX><CY>42</CY></Size>
            <Background>A_Corner</Background><EQType>1</EQType>
        </InvSlot>
        <Screen item="Figure"><Location><X>166</X><Y>57</Y></Location>
            <Size><CX>85</CX><CY>168</CY></Size><DrawTemplate>WDT_Plain</DrawTemplate>
            <Style_Border>true</Style_Border>
            <TooltipReference>Drop Item Here to Auto Equip</TooltipReference>
        </Screen>
        <Page item="FirstPage"><TabText>Inventory</TabText>
            <Location><X>0</X><Y>22</Y></Location><Size><CX>388</CX><CY>401</CY></Size>
            <Pieces>Ear</Pieces><Pieces>Screen:Figure</Pieces>
        </Page>
        <TabBox item="Tabs"><Pages>Page:FirstPage</Pages></TabBox>
        <Button item="Platinum"><ScreenID>IW_Money0</ScreenID>
            <Location><X>303</X><Y>121</Y></Location><Size><CX>70</CX><CY>24</CY></Size>
            <Text>9999</Text><TextColor><R>255</R><G>255</G><B>255</B></TextColor>
            <ButtonDrawTemplate><Normal>A_Back</Normal><NormalDecal>A_Corner</NormalDecal></ButtonDrawTemplate>
            <DecalOffset><X>1</X><Y>3</Y></DecalOffset><DecalSize><CX>18</CX><CY>18</CY></DecalSize>
        </Button>
        <Screen item="Bags"><Size><CX>428</CX><CY>460</CY></Size>
            <Pieces>Tabs</Pieces><Pieces>Platinum</Pieces>
        </Screen>
        <Screen item="SampleWindow">
            <Location><X>516</X><Y>242</Y></Location><Size><CX>147</CX><CY>50</CY></Size>
            <TooltipReference>Your Current Target</TooltipReference>
            <DrawTemplate>WDT_Plain</DrawTemplate>
            <Style_Titlebar>true</Style_Titlebar><Style_Border>true</Style_Border>
            <Pieces>Health</Pieces><Pieces>Percent</Pieces><Pieces>BoxPicture</Pieces>
            <Pieces>Gem0</Pieces><Pieces>Book</Pieces>
            <Pieces>Undefined</Pieces>
        </Screen>
    </XML>"#;

    #[test]
    fn a_window_lists_its_pieces_with_their_geometry_and_chrome() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let screen = library.screen(WINDOW, "SampleWindow").unwrap();
        assert_eq!(
            screen.area,
            Area {
                x: 516.0,
                y: 242.0,
                width: 147.0,
                height: 50.0
            }
        );
        assert!(screen.titlebar && screen.border);
        assert_eq!(screen.tooltip.as_deref(), Some("Your Current Target"));
        let template = screen.template.unwrap();
        assert_eq!(template.background.as_deref(), Some("rock.tga"));
        assert_eq!(template.border.top_left.unwrap().texture, "frame.tga");
        // A piece the library lacks is simply not drawn.
        assert!(template.border.top.is_none());
        assert_eq!(template.title[1].as_ref().unwrap().y, 7);
        let names: Vec<_> = screen
            .pieces
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["Health", "Percent", "BoxPicture", "Gem0", "Book"]);
        let Element::Gauge(gauge) = &screen.pieces[0].1 else {
            panic!("a gauge")
        };
        assert_eq!(gauge.eq_type, Some(6));
        assert_eq!(gauge.fill_tint, Some([240, 0, 0]));
        assert_eq!(gauge.look.fill.as_ref().unwrap().y, 18);
        assert_eq!((gauge.text_offset, gauge.bar_offset), ((8.0, 0.0), 16.0));
        let Element::Label(label) = &screen.pieces[1].1 else {
            panic!("a label")
        };
        assert_eq!(
            (label.eq_type, label.align, label.font),
            (Some(29), Align::Right, Some(1))
        );
        // The window file's own pieces come with it, sized by their piece.
        let Element::Image { area, piece, .. } = &screen.pieces[2].1 else {
            panic!("an image")
        };
        assert_eq!(
            (area.width, area.height, piece.texture.as_str()),
            (116.0, 30.0, "box.tga")
        );
    }

    #[test]
    fn gems_and_buttons_keep_their_names_and_pieces() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let screen = library.screen(WINDOW, "SampleWindow").unwrap();
        let Element::SpellGem(gem) = &screen.pieces[3].1 else {
            panic!("a gem")
        };
        assert_eq!(gem.id.as_deref(), Some("CSPW_Spell0"));
        assert_eq!(gem.holder.as_ref().unwrap().y, 7);
        let Element::Button(button) = &screen.pieces[4].1 else {
            panic!("a button")
        };
        assert!(button.checkbox);
        assert_eq!(button.look.normal.as_ref().unwrap().y, 18);
        assert!(button.look.flyby.is_none());
        assert_eq!(
            button.tooltip.as_deref(),
            Some("Opens and closes Your Spellbook")
        );
    }

    #[test]
    fn pages_hold_slots_and_windows_within_windows() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let screen = library.screen(WINDOW, "Bags").unwrap();
        let Element::Tabs(pages) = &screen.pieces[0].1 else {
            panic!("tabs")
        };
        let page = &pages[0];
        assert_eq!(page.title.as_deref(), Some("Inventory"));
        assert_eq!(
            page.area,
            Some(Area {
                x: 0.0,
                y: 22.0,
                width: 388.0,
                height: 401.0
            })
        );
        let Element::InvSlot(slot) = &page.pieces[0].1 else {
            panic!("a slot")
        };
        assert_eq!((slot.slot, slot.area.x), (Some(1), 123.0));
        assert_eq!(slot.background.as_ref().unwrap().texture, "frame.tga");
        let Element::View(view) = &page.pieces[1].1 else {
            panic!("a view")
        };
        assert_eq!(view.name, "Figure");
        assert!(view.border && view.template.is_some());
        assert_eq!(
            view.tooltip.as_deref(),
            Some("Drop Item Here to Auto Equip")
        );
        let Element::Button(money) = &screen.pieces[1].1 else {
            panic!("a button")
        };
        assert_eq!(money.decal.as_ref().unwrap().texture, "frame.tga");
        assert_eq!(
            money.decal_area,
            Some(Area {
                x: 1.0,
                y: 3.0,
                width: 18.0,
                height: 18.0
            })
        );
        assert_eq!(money.text_color, Some([255, 255, 255]));
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn the_installed_skin_defines_the_player_and_target_windows() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let install = Path::new(&install);
        let library = Library::read(install, "default").unwrap();
        for (file, name) in [
            ("EQUI_PlayerWindow.xml", "PlayerWindow"),
            ("EQUI_TargetWindow.xml", "TargetWindow"),
            ("EQUI_Inventory.xml", "InventoryWindow"),
        ] {
            let screen = library.window(install, "default", file, name).unwrap();
            // The inventory keeps its gauges on its first page.
            let pieces: Vec<&Element> = screen
                .pieces
                .iter()
                .flat_map(|(_, element)| match element {
                    Element::Tabs(pages) => pages
                        .first()
                        .map(|page| page.pieces.iter().map(|(_, piece)| piece).collect())
                        .unwrap_or_default(),
                    other => vec![other],
                })
                .collect();
            let gauges: Vec<_> = pieces
                .iter()
                .filter_map(|element| match element {
                    Element::Gauge(gauge) => gauge.eq_type,
                    _ => None,
                })
                .collect();
            println!("{name}: {:?} gauges {gauges:?}", screen.area);
            assert!(!gauges.is_empty(), "{name} shows a gauge");
            assert!(screen.template.is_some(), "{name} names its chrome");
        }
    }

    #[test]
    fn a_file_without_the_window_says_so() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        assert!(matches!(
            library.screen(WINDOW, "OtherWindow"),
            Err(UiLayoutError::MissingWindow(name)) if name == "OtherWindow"
        ));
    }
}
