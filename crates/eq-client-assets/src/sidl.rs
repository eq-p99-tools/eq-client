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

/// A frame drawn around a box from pieces (`FrameTemplate`), as a tab's,
/// a page's or a list's column heading's: its corners and edges, and the
/// caps at the ends of its sides. A frame open at the bottom, as a tab's,
/// has no bottom pieces; a heading's is its left end, its middle,
/// stretched, and its right end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameLook {
    /// The top left corner.
    pub top_left: Option<Piece>,
    /// The top edge, stretched between the corners.
    pub top: Option<Piece>,
    /// The top right corner.
    pub top_right: Option<Piece>,
    /// The left side's top end, below the top left corner.
    pub left_top: Option<Piece>,
    /// The left side, stretched between its ends; a heading's left end.
    pub left: Option<Piece>,
    /// The left side's bottom end.
    pub left_bottom: Option<Piece>,
    /// The right side's top end.
    pub right_top: Option<Piece>,
    /// The right side, stretched between its ends; a heading's right end.
    pub right: Option<Piece>,
    /// The right side's bottom end.
    pub right_bottom: Option<Piece>,
    /// The bottom left corner.
    pub bottom_left: Option<Piece>,
    /// The bottom edge, stretched between the corners.
    pub bottom: Option<Piece>,
    /// The bottom right corner.
    pub bottom_right: Option<Piece>,
    /// A heading's middle, stretched between its ends.
    pub middle: Option<Piece>,
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
    /// The title bar's close box, in each of its states.
    pub close_box: Option<ButtonLook>,
    /// The title bar's minimize box, in each of its states.
    pub minimize_box: Option<ButtonLook>,
    /// The vertical scrollbar of a box drawn with it that scrolls.
    pub scrollbar: Option<ScrollbarLook>,
}

/// How a vertical scrollbar is drawn: its two arrows, its thumb and the
/// gutter the thumb runs in between them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScrollbarLook {
    /// The arrow at the top, in each of its states.
    pub up: ButtonLook,
    /// The arrow at the bottom, in each of its states.
    pub down: ButtonLook,
    /// The thumb's top, its middle, stretched, and its bottom.
    pub thumb: [Option<Piece>; 3],
    /// The texture stretched along the gutter, in the skin's directory.
    pub gutter: Option<String>,
    /// The colour the gutter's texture is tinted with.
    pub gutter_tint: Option<[u8; 3]>,
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
    /// How far down the gauge the bar sits (`GaugeOffsetY`), 16 where the
    /// skin leaves it out. Below zero, the bar starts above the gauge, and a
    /// thin gauge shows only the rows of it inside, as the default skin's
    /// pet line does.
    pub bar_offset: f32,
    /// Where the bar starts from the gauge's left (`GaugeOffsetX`): its left
    /// end, then the bar its fill grows along, each piece at its own size.
    /// Skins set it below zero to show only part of a bar, as the Velious
    /// skin colours each fifth of its hit point bar with a gauge over that
    /// fifth alone.
    pub bar_left: f32,
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
    /// Whether the skin gives its place (`Location`); some skins leave the
    /// effects windows' buttons for the client to place.
    pub placed: bool,
    /// Where it sits when it stretches with its window instead
    /// (`AutoStretch`), such as a Done button kept to the bottom corner.
    pub anchors: Option<Anchors>,
    /// How it is drawn.
    pub look: ButtonLook,
    /// Whether it stays on once pressed, as a window's toggle does.
    pub checkbox: bool,
    /// Its words, if it has any.
    pub text: Option<String>,
    /// Its words' colour.
    pub text_color: Option<[u8; 3]>,
    /// The official client's font number for its words, from 1 for the
    /// smallest, where the skin gives one.
    pub font: Option<u8>,
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
    /// The colour of its tab's words, and while it is the page shown.
    pub title_colors: [Option<[u8; 3]>; 2],
    /// What its tab says under the pointer.
    pub tooltip: Option<String>,
}

/// Pages behind tabs, one shown at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct TabBox {
    /// What the window calls it.
    pub name: String,
    /// Where it sits in its container; None where the skin stretches it
    /// over the container.
    pub area: Option<Area>,
    /// Its pages, the first shown until another is chosen.
    pub pages: Vec<Page>,
    /// The frame around each tab (`TabBorderTemplate`).
    pub tab_frame: Option<FrameLook>,
    /// The frame around the page shown (`PageBorderTemplate`).
    pub page_frame: Option<FrameLook>,
}

/// A window inside a window, such as the inventory's character view or the
/// pet window's buffs.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// What the window calls it, such as `IW_CharacterView`.
    pub name: String,
    /// Where it sits in its window.
    pub area: Area,
    /// Where it sits when it stretches with its window instead
    /// (`AutoStretch`).
    pub anchors: Option<Anchors>,
    /// How its frame is drawn.
    pub template: Option<WindowTemplate>,
    /// Whether it has a border.
    pub border: bool,
    /// The tooltip the skin gives it.
    pub tooltip: Option<String>,
    /// What it shows, in drawing order, by element name.
    pub pieces: Vec<(String, Element)>,
    /// Its vertical scrollbar, where the skin gives it one
    /// (`Style_VScroll`), as the loot window's slots scroll.
    pub scrollbar: Option<ScrollbarLook>,
}

/// Where an element that stretches with its container sits: each edge's
/// distance from the container's edge it keeps to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchors {
    /// The left edge.
    pub left: Anchor,
    /// The top edge.
    pub top: Anchor,
    /// The right edge.
    pub right: Anchor,
    /// The bottom edge.
    pub bottom: Anchor,
}

/// One edge of a stretching element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// This far from the container's left or top.
    Start(f32),
    /// This far from the container's right or bottom.
    End(f32),
}

impl Anchor {
    /// Where the edge lies along a container of this length.
    #[must_use]
    pub const fn along(self, length: f32) -> f32 {
        match self {
            Self::Start(offset) => offset,
            Self::End(offset) => length - offset,
        }
    }
}

impl Anchors {
    /// The area these anchors give inside a container of this size.
    #[must_use]
    pub fn within(&self, width: f32, height: f32) -> Area {
        let (left, right) = (self.left.along(width), self.right.along(width));
        let (top, bottom) = (self.top.along(height), self.bottom.along(height));
        Area {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        }
    }
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

/// How a slider is drawn: its track between two end caps, and its thumb
/// in each state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SliderLook {
    /// The track, stretched between the end caps.
    pub background: Option<Piece>,
    /// The track's left end.
    pub cap_left: Option<Piece>,
    /// The track's right end.
    pub cap_right: Option<Piece>,
    /// The thumb the player drags.
    pub thumb: ButtonLook,
}

/// A slider: a thumb the player drags along a track to set a value.
#[derive(Clone, Debug, PartialEq)]
pub struct Slider {
    /// What the window calls it, such as `ODP_ClipPlaneSlider`.
    pub id: Option<String>,
    /// Where it sits in its window.
    pub area: Area,
    /// How it is drawn.
    pub look: SliderLook,
}

/// A drop-down: the choice made, and a button that opens the list of
/// choices.
#[derive(Clone, Debug, PartialEq)]
pub struct Combobox {
    /// What the window calls it, such as `ODP_SkyCombobox`.
    pub id: Option<String>,
    /// Where it sits in its window.
    pub area: Area,
    /// How its frame is drawn.
    pub template: Option<WindowTemplate>,
    /// The button that opens the list.
    pub button: ButtonLook,
    /// What may be chosen, in order.
    pub choices: Vec<String>,
    /// How tall the open list is.
    pub list_height: f32,
}

/// One column of a list: its heading and width.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    /// The column's heading.
    pub heading: String,
    /// The column's width.
    pub width: f32,
    /// A picture that heads the column instead of words, from a frame
    /// template's middle, as each coin heads a price's columns.
    pub header: Option<FrameLook>,
}

/// A list of rows in columns under headings, such as the Options window's
/// key assignments.
#[derive(Clone, Debug, PartialEq)]
pub struct Listbox {
    /// What the window calls it, such as `OKP_KeyboardAssignmentList`.
    pub id: Option<String>,
    /// Where it sits in its window, when the skin places it.
    pub area: Option<Area>,
    /// Where it sits when it stretches with its window instead
    /// (`AutoStretch`).
    pub anchors: Option<Anchors>,
    /// How its frame is drawn.
    pub template: Option<WindowTemplate>,
    /// Its columns, from the left.
    pub columns: Vec<Column>,
    /// The official client's font number for its rows, where the skin gives
    /// one.
    pub font: Option<u8>,
    /// Its vertical scrollbar, where the skin gives it one
    /// (`Style_VScroll`).
    pub scrollbar: Option<ScrollbarLook>,
    /// How its columns' headings are drawn: the skin's `Header_Listbox`
    /// frame. No list names it; that every list's headings take it is
    /// inferred from the template's name, unchecked against the official
    /// client.
    pub header: Option<Box<FrameLook>>,
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
    /// Where it sits when it stretches with its window instead
    /// (`AutoStretch`), such as the casting window's spell name.
    pub anchors: Option<Anchors>,
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
    /// Whether its words wrap onto more lines (`NoWrap` false); one that
    /// says nothing keeps to one line (unchecked).
    pub wraps: bool,
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
    /// A slider.
    Slider(Slider),
    /// A drop-down.
    Combobox(Combobox),
    /// A list in columns.
    Listbox(Listbox),
    /// Pages behind tabs; the first is shown.
    Tabs(TabBox),
    /// A window inside the window.
    View(Box<View>),
    /// A box of text the client fills, such as a confirmation's question
    /// (`STMLbox`).
    TextBox(TextBox),
    /// An element this reader does not draw yet, by its kind.
    Other(String),
}

/// A box of text the client fills.
#[derive(Clone, Debug, PartialEq)]
pub struct TextBox {
    /// What the window calls it (`ScreenID`), such as `TextOutput`.
    pub id: Option<String>,
    /// Where it sits in its window, when the skin places it.
    pub area: Area,
    /// Where it sits when it stretches with its window instead
    /// (`AutoStretch`).
    pub anchors: Option<Anchors>,
    /// How its frame is drawn; None for a box without one, such as a page
    /// of a book, drawn on the window's own art.
    pub template: Option<WindowTemplate>,
    /// The colour of its text, where the skin sets one.
    pub color: Option<[u8; 3]>,
    /// The official client's font number for its text, where the skin gives
    /// one.
    pub font: Option<u8>,
    /// Its vertical scrollbar, where the skin gives it one
    /// (`Style_VScroll`).
    pub scrollbar: Option<ScrollbarLook>,
}

/// A window's title bar, and the boxes on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TitleBar {
    /// Whether it has a close box.
    pub close_box: bool,
    /// Whether it has a minimize box.
    pub minimize_box: bool,
}

/// A window as the skin defines it.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    /// The window's name, such as `PlayerWindow`.
    pub name: String,
    /// The words on its title bar (`Text`), where the skin gives some.
    pub title: Option<String>,
    /// The colour of its title, where the skin sets one.
    pub title_color: Option<[u8; 3]>,
    /// The official client's font number for its title, from 1 for the
    /// smallest.
    pub font: Option<u8>,
    /// Its default place and size on an 800 by 600 screen, borders and
    /// title bar included.
    pub area: Area,
    /// How its frame is drawn.
    pub template: Option<WindowTemplate>,
    /// Its title bar, where it has one.
    pub title_bar: Option<TitleBar>,
    /// Whether it has a border.
    pub border: bool,
    /// Whether the skin draws no background for it (`Style_Transparent`),
    /// so what lies behind shows between its pieces: inferred from the
    /// setting's name, not checked against the official client.
    pub transparent: bool,
    /// The tooltip the skin gives it.
    pub tooltip: Option<String>,
    /// What it shows, in drawing order, by element name.
    pub pieces: Vec<(String, Element)>,
    /// The skin's frame for tabs the client draws in it that its file
    /// defines none of, as the chat's channel tabs: `FT_DefTabBorder`, the
    /// frame every tab box names in the six skins installed on the PC this
    /// was written on (counted); unchecked against the official client.
    pub tab_frame: Option<Box<FrameLook>>,
}

/// The pieces and window templates a skin's windows refer to by name.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pieces: HashMap<String, Piece>,
    templates: HashMap<String, WindowTemplate>,
    sliders: HashMap<String, SliderLook>,
    buttons: HashMap<String, ButtonLook>,
    frames: HashMap<String, FrameLook>,
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
        let documents = [
            roxmltree::Document::parse(animations)?,
            roxmltree::Document::parse(templates)?,
        ];
        Ok(Self::of(&documents))
    }

    /// The pieces and templates these documents define, the pieces first,
    /// since the templates name them.
    fn of(documents: &[roxmltree::Document<'_>]) -> Self {
        let mut library = Self::default();
        for document in documents {
            library.add_pieces(document);
        }
        for document in documents {
            library.add_templates(document);
        }
        library
    }

    /// Notes every template in a document by name.
    fn add_templates(&mut self, document: &roxmltree::Document<'_>) {
        let library = self;
        for node in document.root_element().children() {
            let Some(name) = node.attribute("item") else {
                continue;
            };
            match node.tag_name().name() {
                "WindowDrawTemplate" => {
                    let template = library.template(node);
                    library.templates.insert(name.to_owned(), template);
                }
                "SliderDrawTemplate" => {
                    let look = library.slider_look(node);
                    library.sliders.insert(name.to_owned(), look);
                }
                "ButtonDrawTemplate" => {
                    let look = library.button_look(Some(node));
                    library.buttons.insert(name.to_owned(), look);
                }
                "FrameTemplate" => {
                    let look = library.frame_look(node);
                    library.frames.insert(name.to_owned(), look);
                }
                _ => (),
            }
        }
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
        self.screen_among(&[roxmltree::Document::parse(text)?], name)
    }

    /// The screen of this name in the first of these documents. Its pieces
    /// are found in that document first, then in the others, in order: the
    /// files of one set, which the official client reads into one namespace.
    fn screen_among(
        &self,
        documents: &[roxmltree::Document<'_>],
        name: &str,
    ) -> Result<Screen, UiLayoutError> {
        // The screen's own file's pictures win over the library's and the
        // other files', so it comes last.
        let mut local = self.clone();
        for document in documents.iter().rev() {
            local.add_pieces(document);
        }
        let named: Vec<HashMap<&str, roxmltree::Node<'_, '_>>> = documents
            .iter()
            .map(|document| {
                document
                    .root_element()
                    .children()
                    .filter_map(|node| Some((node.attribute("item")?, node)))
                    .collect()
            })
            .collect();
        let screen = named
            .first()
            .and_then(|own| own.get(name))
            .filter(|node| node.has_tag_name("Screen"))
            .copied()
            .ok_or_else(|| UiLayoutError::MissingWindow(name.to_owned()))?;
        let mut elements = HashMap::new();
        for file in &named {
            for (item, node) in file {
                elements.entry(*item).or_insert(*node);
            }
        }
        let pieces = local.pieces(screen, &elements, 0);
        Ok(Screen {
            name: name.to_owned(),
            title: text_of(screen, "Text").map(str::to_owned),
            title_color: color(screen, "TextColor"),
            font: number(screen, "Font"),
            area: area(screen).unwrap_or(Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            }),
            template: text_of(screen, "DrawTemplate")
                .and_then(|template| local.templates.get(template).cloned()),
            title_bar: flag(screen, "Style_Titlebar").then(|| TitleBar {
                close_box: flag(screen, "Style_Closebox"),
                minimize_box: flag(screen, "Style_Minimizebox"),
            }),
            border: flag(screen, "Style_Border"),
            transparent: flag(screen, "Style_Transparent"),
            tooltip: text_of(screen, "TooltipReference").map(str::to_owned),
            pieces,
            tab_frame: local.frames.get("FT_DefTabBorder").cloned().map(Box::new),
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

    /// One of the skin's pictures by its animation's name, as its first
    /// frame, such as `A_PlatinumCoin`.
    #[must_use]
    pub fn named_piece(&self, name: &str) -> Option<Piece> {
        self.piece(Some(name))
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
            close_box: child(node, "CloseBox").map(|look| self.button_look(Some(look))),
            minimize_box: child(node, "MinimizeBox").map(|look| self.button_look(Some(look))),
            scrollbar: child(node, "VSBTemplate").map(|look| self.scrollbar_look(look)),
        }
    }

    /// A frame's pictures, from a `FrameTemplate`.
    fn frame_look(&self, node: roxmltree::Node<'_, '_>) -> FrameLook {
        let part = |name: &str| self.piece(text_of(node, name));
        FrameLook {
            top_left: part("TopLeft"),
            top: part("Top"),
            top_right: part("TopRight"),
            left_top: part("LeftTop"),
            left: part("Left"),
            left_bottom: part("LeftBottom"),
            right_top: part("RightTop"),
            right: part("Right"),
            right_bottom: part("RightBottom"),
            bottom_left: part("BottomLeft"),
            bottom: part("Bottom"),
            bottom_right: part("BottomRight"),
            middle: part("Middle"),
        }
    }

    /// The frame a tab box names for its tabs or pages.
    fn named_frame(&self, node: roxmltree::Node<'_, '_>, name: &str) -> Option<FrameLook> {
        self.frames.get(text_of(node, name)?).cloned()
    }

    /// A vertical scrollbar's pictures, from a template's `VSBTemplate`.
    fn scrollbar_look(&self, node: roxmltree::Node<'_, '_>) -> ScrollbarLook {
        let thumb = child(node, "Thumb");
        let part = |name: &str| self.piece(thumb.and_then(|thumb| text_of(thumb, name)));
        ScrollbarLook {
            up: self.button_look(child(node, "UpButton")),
            down: self.button_look(child(node, "DownButton")),
            thumb: [part("Top"), part("Middle"), part("Bottom")],
            gutter: text_of(node, "MiddleTextureInfo").map(str::to_owned),
            gutter_tint: color(node, "MiddleTint"),
        }
    }

    /// The scrollbar a box asks for (`Style_VScroll`), as its template draws
    /// it, though the box may draw none of its frame.
    fn scrollbar(&self, node: roxmltree::Node<'_, '_>) -> Option<ScrollbarLook> {
        if !flag(node, "Style_VScroll") {
            return None;
        }
        self.templates
            .get(text_of(node, "DrawTemplate")?)?
            .scrollbar
            .clone()
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

    /// A button's pictures in each state, from a `ButtonDrawTemplate` or a
    /// slider's `Thumb`.
    fn button_look(&self, look: Option<roxmltree::Node<'_, '_>>) -> ButtonLook {
        let state = |name: &str| self.piece(look.and_then(|look| text_of(look, name)));
        ButtonLook {
            normal: state("Normal"),
            pressed: state("Pressed"),
            flyby: state("Flyby"),
            disabled: state("Disabled"),
            pressed_flyby: state("PressedFlyby"),
        }
    }

    /// A slider's pictures, from a `SliderDrawTemplate`.
    fn slider_look(&self, node: roxmltree::Node<'_, '_>) -> SliderLook {
        SliderLook {
            background: self.piece(text_of(node, "Background")),
            cap_left: self.piece(text_of(node, "EndCapLeft")),
            cap_right: self.piece(text_of(node, "EndCapRight")),
            thumb: self.button_look(child(node, "Thumb")),
        }
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
            placed: child(node, "Location").is_some(),
            anchors: flag(node, "AutoStretch").then(|| anchors(node)),
            look: self.button_look(look),
            checkbox: flag(node, "Style_Checkbox"),
            text: text_of(node, "Text").map(str::to_owned),
            text_color: color(node, "TextColor"),
            font: number(node, "Font"),
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
                    title_colors: [
                        color(*page, "TabTextColor"),
                        color(*page, "TabTextActiveColor"),
                    ],
                    tooltip: text_of(*page, "TooltipReference").map(str::to_owned),
                })
            })
            .collect()
    }

    /// A gauge: a bar showing a fraction, with its text.
    fn gauge(&self, node: roxmltree::Node<'_, '_>) -> Element {
        let at = || {
            area(node).unwrap_or(Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            })
        };
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
            // The skin schema's default (SIDL.xml), which the default skin's
            // group window gauges leave to it.
            bar_offset: number(node, "GaugeOffsetY").unwrap_or(16.0),
            bar_left: number(node, "GaugeOffsetX").unwrap_or(0.0),
        })
    }

    /// A slider, a drop-down or a list.
    fn control(&self, node: roxmltree::Node<'_, '_>) -> Element {
        let at = || {
            area(node).unwrap_or(Area {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            })
        };
        let id = || text_of(node, "ScreenID").map(str::to_owned);
        let template = || {
            text_of(node, "DrawTemplate").and_then(|template| self.templates.get(template).cloned())
        };
        match node.tag_name().name() {
            "Slider" => Element::Slider(Slider {
                id: id(),
                area: at(),
                look: text_of(node, "SliderArt")
                    .and_then(|art| self.sliders.get(art).cloned())
                    .unwrap_or_default(),
            }),
            "Combobox" => Element::Combobox(Combobox {
                id: id(),
                area: at(),
                template: template(),
                button: text_of(node, "Button")
                    .and_then(|button| self.buttons.get(button).cloned())
                    .unwrap_or_default(),
                choices: node
                    .children()
                    .filter(|child| child.has_tag_name("Choices"))
                    .filter_map(|child| child.text())
                    .map(|choice| choice.trim().to_owned())
                    .collect(),
                list_height: number(node, "ListHeight").unwrap_or(100.0),
            }),
            "Listbox" => Element::Listbox(Listbox {
                id: id(),
                area: area(node),
                anchors: flag(node, "AutoStretch").then(|| anchors(node)),
                template: template(),
                columns: node
                    .children()
                    .filter(|child| child.has_tag_name("Columns"))
                    .map(|column| Column {
                        heading: text_of(column, "Heading").unwrap_or_default().to_owned(),
                        width: number(column, "Width").unwrap_or(80.0),
                        header: self.named_frame(column, "Header"),
                    })
                    .collect(),
                font: number(node, "Font"),
                scrollbar: self.scrollbar(node),
                header: self.frames.get("Header_Listbox").cloned().map(Box::new),
            }),
            other => Element::Other(other.to_owned()),
        }
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
            "Gauge" => self.gauge(node),
            // Words the skin writes, or the client fills: StaticText is how
            // the skin writes a book's page numbers.
            "Label" | "StaticText" => Element::Label(Label {
                area: at(),
                anchors: flag(node, "AutoStretch").then(|| anchors(node)),
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
                wraps: text_of(node, "NoWrap")
                    .is_some_and(|text| text.eq_ignore_ascii_case("false")),
            }),
            "Button" => self.button(node),
            "InvSlot" => Element::InvSlot(InvSlot {
                id: text_of(node, "ScreenID").map(str::to_owned),
                area: at(),
                slot: number(node, "EQType"),
                background: self.piece(text_of(node, "Background")),
            }),
            "Slider" | "Combobox" | "Listbox" => self.control(node),
            // A box of text, or a field the client fills with text, as a
            // book's pages are.
            "STMLbox" | "Editbox" => Element::TextBox(TextBox {
                id: text_of(node, "ScreenID").map(str::to_owned),
                area: at(),
                anchors: flag(node, "AutoStretch").then(|| anchors(node)),
                template: text_of(node, "DrawTemplate")
                    .filter(|_| {
                        !flag(node, "Style_Transparent")
                            && text_of(node, "Style_Border") != Some("false")
                    })
                    .and_then(|template| self.templates.get(template).cloned()),
                color: color(node, "TextColor"),
                font: number(node, "Font"),
                scrollbar: self.scrollbar(node),
            }),
            // Pages and windows within windows nest; skins go a few deep.
            "TabBox" if depth < 4 => Element::Tabs(TabBox {
                name: node.attribute("item").unwrap_or_default().to_owned(),
                area: area(node).filter(|_| !flag(node, "AutoStretch")),
                pages: self.pages(node, elements, depth),
                tab_frame: self.named_frame(node, "TabBorderTemplate"),
                page_frame: self.named_frame(node, "PageBorderTemplate"),
            }),
            "Screen" => Element::View(Box::new(View {
                name: node.attribute("item").unwrap_or_default().to_owned(),
                area: at(),
                anchors: flag(node, "AutoStretch").then(|| anchors(node)),
                template: text_of(node, "DrawTemplate")
                    .and_then(|template| self.templates.get(template).cloned()),
                border: flag(node, "Style_Border"),
                tooltip: text_of(node, "TooltipReference").map(str::to_owned),
                pieces: if depth < 4 {
                    self.pieces(node, elements, depth + 1)
                } else {
                    Vec::new()
                },
                scrollbar: self.scrollbar(node),
            })),
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

/// The index of the login front end's set of interface files, which names
/// the others.
pub const LOGIN_SET: &str = "EQLSUI.xml";

/// One of the installed client's sets of interface files, read whole, as
/// the official client reads every file a set's index names into one
/// namespace: a screen may list a piece another of the set's files defines,
/// as the login screens list their backgrounds.
#[derive(Clone, Debug, Default)]
pub struct InterfaceSet {
    /// The pictures and templates the set's files define.
    library: Library,
    /// Each file's name and text, in the order the index names them.
    files: Vec<(String, String)>,
}

impl InterfaceSet {
    /// Reads the set an index file names, such as [`LOGIN_SET`], each file
    /// from the skin or else the default skin. A file named that neither
    /// has, or that is not well-formed, is left out, as it defines nothing
    /// this client can use.
    ///
    /// # Errors
    /// Rejects unsafe names, and an unreadable or malformed index.
    pub fn read(eq_directory: &Path, skin: &str, index: &str) -> Result<Self, UiLayoutError> {
        let index = read_text(eq_directory, skin, index)?;
        let document = roxmltree::Document::parse(&index)?;
        let files = document
            .descendants()
            .filter(|node| node.has_tag_name("Include"))
            .filter_map(|node| node.text())
            .map(str::trim)
            .filter_map(|file| Some((file.to_owned(), read_text(eq_directory, skin, file).ok()?)))
            .collect();
        Ok(Self::parse(files))
    }

    /// The set of these files, by name, in the index's order. A file that
    /// is not well-formed is left out.
    #[must_use]
    pub fn parse(files: Vec<(String, String)>) -> Self {
        let files: Vec<(String, String)> = files
            .into_iter()
            .filter(|(_, text)| roxmltree::Document::parse(text).is_ok())
            .collect();
        let documents: Vec<_> = files
            .iter()
            .filter_map(|(_, text)| roxmltree::Document::parse(text).ok())
            .collect();
        Self {
            library: Library::of(&documents),
            files,
        }
    }

    /// The screen of this name in one of the set's files. Its pieces are
    /// found in that file first, then in the set's other files, in the
    /// index's order.
    ///
    /// # Errors
    /// Rejects a file the set lacks, and one that defines no such screen.
    pub fn screen(&self, file: &str, name: &str) -> Result<Screen, UiLayoutError> {
        let own = self
            .files
            .iter()
            .position(|(named, _)| named.eq_ignore_ascii_case(file))
            .ok_or_else(|| UiLayoutError::MissingFile(file.to_owned()))?;
        let documents: Vec<_> = std::iter::once(&self.files[own])
            .chain(
                self.files
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index != own)
                    .map(|(_, file)| file),
            )
            .map(|(_, text)| roxmltree::Document::parse(text))
            .collect::<Result<_, _>>()?;
        self.library.screen_among(&documents, name)
    }
}

/// A window file's text, from the skin or the default skin, decoded leniently:
/// skins declare ASCII, but hand-edited ones may carry Latin-1 text.
fn read_text(eq_directory: &Path, skin: &str, file: &str) -> Result<String, UiLayoutError> {
    let path = skin_file(eq_directory, skin, file)?;
    let bytes = std::fs::read(&path).map_err(|source| UiLayoutError::Read { path, source })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// An element's setting of this name. A setting an element gives twice
/// counts as the first time: skins do that (the default skin's loot window
/// says it has a title bar, then that it has none; Velious's Actions window
/// right-aligns its stat numbers, then not; four elements in the default skin
/// and thirteen in Velious on the PC this was written on, counted), and which
/// of the two the official client uses is not checked yet.
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

/// A stretching element's anchors; an edge keeps to the container's top or
/// left unless the skin says otherwise.
fn anchors(node: roxmltree::Node<'_, '_>) -> Anchors {
    let edge = |offset: &str, to_start: &str| {
        let offset = number(node, offset).unwrap_or(0.0);
        if text_of(node, to_start).is_none_or(|text| text.eq_ignore_ascii_case("true")) {
            Anchor::Start(offset)
        } else {
            Anchor::End(offset)
        }
    };
    Anchors {
        left: edge("LeftAnchorOffset", "LeftAnchorToLeft"),
        top: edge("TopAnchorOffset", "TopAnchorToTop"),
        right: edge("RightAnchorOffset", "RightAnchorToLeft"),
        bottom: edge("BottomAnchorOffset", "BottomAnchorToTop"),
    }
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
            <CloseBox><Normal>A_Corner</Normal><Pressed>A_Fill</Pressed></CloseBox>
            <VSBTemplate>
                <UpButton><Normal>A_Corner</Normal><Pressed>A_Fill</Pressed></UpButton>
                <DownButton><Normal>A_Back</Normal></DownButton>
                <Thumb><Top>A_Corner</Top><Middle>A_Fill</Middle><Bottom>A_Back</Bottom></Thumb>
                <MiddleTextureInfo>gutter.tga</MiddleTextureInfo>
                <MiddleTint><Alpha>255</Alpha><R>128</R><G>128</G><B>128</B></MiddleTint>
            </VSBTemplate>
        </WindowDrawTemplate>
        <SliderDrawTemplate item="SDT_Plain">
            <Thumb><Normal>A_Fill</Normal><Disabled>A_Corner</Disabled></Thumb>
            <Background>A_Back</Background>
            <EndCapLeft>A_Corner</EndCapLeft><EndCapRight>A_Corner</EndCapRight>
        </SliderDrawTemplate>
        <ButtonDrawTemplate item="BDT_Down">
            <Normal>A_Corner</Normal><Pressed>A_Fill</Pressed>
        </ButtonDrawTemplate>
        <FrameTemplate item="FT_Tab">
            <TopLeft>A_Corner</TopLeft><Top>A_Fill</Top><TopRight>A_Corner</TopRight>
            <LeftTop>A_Back</LeftTop><Left>A_Fill</Left><LeftBottom>A_Corner</LeftBottom>
        </FrameTemplate>
        <FrameTemplate item="FT_Page">
            <BottomLeft>A_Corner</BottomLeft><Bottom>A_Back</Bottom>
        </FrameTemplate>
        <FrameTemplate item="Header_Listbox">
            <Left>A_Corner</Left><Middle>A_Back</Middle><Right>A_Corner</Right>
        </FrameTemplate>
    </XML>"#;

    const CONTROLS: &str = r#"<XML>
        <Slider item="Clip"><ScreenID>ODP_ClipPlaneSlider</ScreenID>
            <Location><X>200</X><Y>129</Y></Location><Size><CX>100</CX><CY>16</CY></Size>
            <SliderArt>SDT_Plain</SliderArt>
        </Slider>
        <Combobox item="Sky"><ScreenID>ODP_SkyCombobox</ScreenID>
            <DrawTemplate>WDT_Plain</DrawTemplate>
            <Location><X>272</X><Y>35</Y></Location><Size><CX>105</CX><CY>24</CY></Size>
            <ListHeight>60</ListHeight><Button>BDT_Down</Button>
            <Choices>Off</Choices><Choices> Simple </Choices><Choices>Complex</Choices>
        </Combobox>
        <Listbox item="Keys"><ScreenID>OKP_KeyboardAssignmentList</ScreenID>
            <DrawTemplate>WDT_Plain</DrawTemplate><AutoStretch>true</AutoStretch>
            <TopAnchorOffset>60</TopAnchorOffset><LeftAnchorOffset>5</LeftAnchorOffset>
            <RightAnchorToLeft>false</RightAnchorToLeft><RightAnchorOffset>5</RightAnchorOffset>
            <BottomAnchorToTop>false</BottomAnchorToTop><BottomAnchorOffset>5</BottomAnchorOffset>
            <Columns><Width>180</Width><Heading>Command</Heading></Columns>
            <Columns><Width>75</Width><Heading>Keypress</Heading></Columns>
            <Style_VScroll>true</Style_VScroll>
        </Listbox>
        <Screen item="Options"><Size><CX>400</CX><CY>300</CY></Size>
            <Pieces>Clip</Pieces><Pieces>Sky</Pieces><Pieces>Keys</Pieces>
        </Screen>
    </XML>"#;

    #[test]
    fn a_column_may_be_headed_by_a_picture() {
        let templates = TEMPLATES.replace(
            "</XML>",
            "<FrameTemplate item=\"Header_Coin\"><Middle>A_Corner</Middle></FrameTemplate></XML>",
        );
        let library = Library::parse(ANIMATIONS, &templates).unwrap();
        let text = r#"<XML>
            <Listbox item="Wares"><Columns><Width>24</Width><Header>Header_Coin</Header></Columns>
                <Columns><Width>150</Width><Heading>Item Name</Heading></Columns></Listbox>
            <Screen item="MerchantWnd"><Pieces>Wares</Pieces></Screen>
        </XML>"#;
        let screen = library.screen(text, "MerchantWnd").unwrap();
        let Element::Listbox(list) = &screen.pieces[0].1 else {
            panic!("a list")
        };
        let middle = list.columns[0]
            .header
            .as_ref()
            .and_then(|look| look.middle.as_ref());
        assert_eq!(middle.map(|piece| piece.width), Some(4));
        assert!(list.columns[1].header.is_none());
    }

    #[test]
    fn a_picture_is_found_by_its_animations_name() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let back = library.named_piece("A_Back").unwrap();
        assert_eq!((back.x, back.y, back.width), (8, 7, 100));
        assert!(library.named_piece("A_Missing").is_none());
    }

    #[test]
    fn sliders_drop_downs_and_lists_carry_their_art_choices_and_columns() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let screen = library.screen(CONTROLS, "Options").unwrap();
        let [
            (_, Element::Slider(slider)),
            (_, Element::Combobox(sky)),
            (_, Element::Listbox(keys)),
        ] = &screen.pieces[..]
        else {
            panic!(
                "expected a slider, a drop-down and a list: {:?}",
                screen.pieces
            );
        };
        assert_eq!(slider.id.as_deref(), Some("ODP_ClipPlaneSlider"));
        assert_eq!((slider.area.x, slider.area.width), (200.0, 100.0));
        assert_eq!(slider.look.background.as_ref().unwrap().y, 7);
        assert_eq!(slider.look.thumb.normal.as_ref().unwrap().y, 18);
        assert!(slider.look.thumb.disabled.is_some() && slider.look.cap_left.is_some());
        assert_eq!(sky.choices, ["Off", "Simple", "Complex"]);
        assert!((sky.list_height - 60.0).abs() < f32::EPSILON);
        assert!(sky.button.pressed.is_some() && sky.template.is_some());
        assert_eq!(keys.area, None);
        let inside = keys.anchors.unwrap().within(400.0, 300.0);
        assert_eq!(
            (inside.x, inside.y, inside.width, inside.height),
            (5.0, 60.0, 390.0, 235.0)
        );
        assert_eq!(
            keys.columns,
            [
                Column {
                    heading: "Command".into(),
                    width: 180.0,
                    header: None,
                },
                Column {
                    heading: "Keypress".into(),
                    width: 75.0,
                    header: None,
                }
            ]
        );
        // The list asks for a scrollbar and gets its template's.
        let scrollbar = keys.scrollbar.as_ref().unwrap();
        assert_eq!(scrollbar.up.normal.as_ref().unwrap().texture, "frame.tga");
        assert_eq!(scrollbar.up.pressed.as_ref().unwrap().y, 18);
        assert_eq!(scrollbar.down.normal.as_ref().unwrap().y, 7);
        assert_eq!(
            scrollbar
                .thumb
                .each_ref()
                .map(|piece| piece.as_ref().map(|piece| piece.y)),
            [Some(0), Some(18), Some(7)]
        );
        assert_eq!(scrollbar.gutter.as_deref(), Some("gutter.tga"));
        assert_eq!(scrollbar.gutter_tint, Some([128, 128, 128]));
        // Its headings take the skin's list heading frame, though it names
        // none.
        let header = keys.header.as_ref().unwrap();
        assert_eq!(header.middle.as_ref().unwrap().y, 7);
        assert_eq!(header.left.as_ref().unwrap().texture, "frame.tga");
    }

    const WINDOW: &str = r#"<?xml version="1.0" encoding="us-ascii"?>
    <XML ID="EQInterfaceDefinitionLanguage">
        <TextureInfo item="box.tga"><Size><CX>128</CX><CY>32</CY></Size></TextureInfo>
        <Ui2DAnimation item="A_Box"><Frames><Texture>box.tga</Texture>
            <Size><CX>116</CX><CY>30</CY></Size></Frames></Ui2DAnimation>
        <StaticAnimation item="BoxPicture"><Animation>A_Box</Animation></StaticAnimation>
        <Gauge item="Health">
            <Location><X>5</X><Y>2</Y></Location><Size><CX>108</CX><CY>27</CY></Size>
            <TextOffsetX>8</TextOffsetX><GaugeOffsetX>20</GaugeOffsetX>
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
            <TabTextActiveColor><R>255</R><G>255</G><B>0</B></TabTextActiveColor>
            <Location><X>0</X><Y>22</Y></Location><Size><CX>388</CX><CY>401</CY></Size>
            <Pieces>Ear</Pieces><Pieces>Screen:Figure</Pieces>
        </Page>
        <TabBox item="Tabs"><Pages>Page:FirstPage</Pages>
            <TabBorderTemplate>FT_Tab</TabBorderTemplate>
            <PageBorderTemplate>FT_Page</PageBorderTemplate>
        </TabBox>
        <Button item="Platinum"><ScreenID>IW_Money0</ScreenID>
            <Location><X>303</X><Y>121</Y></Location><Size><CX>70</CX><CY>24</CY></Size>
            <Text>9999</Text><TextColor><R>255</R><G>255</G><B>255</B></TextColor>
            <ButtonDrawTemplate><Normal>A_Back</Normal><NormalDecal>A_Corner</NormalDecal></ButtonDrawTemplate>
            <DecalOffset><X>1</X><Y>3</Y></DecalOffset><DecalSize><CX>18</CX><CY>18</CY></DecalSize>
        </Button>
        <Button item="Trade"><ScreenID>TRDW_Trade_Button</ScreenID>
            <AutoStretch>true</AutoStretch>
            <LeftAnchorOffset>5</LeftAnchorOffset><TopAnchorOffset>37</TopAnchorOffset>
            <RightAnchorOffset>90</RightAnchorOffset><BottomAnchorOffset>5</BottomAnchorOffset>
            <TopAnchorToTop>false</TopAnchorToTop><BottomAnchorToTop>false</BottomAnchorToTop>
        </Button>
        <Screen item="Bags"><Size><CX>428</CX><CY>460</CY></Size>
            <Pieces>Tabs</Pieces><Pieces>Platinum</Pieces><Pieces>Trade</Pieces>
        </Screen>
        <Screen item="SampleWindow">
            <Location><X>516</X><Y>242</Y></Location><Size><CX>147</CX><CY>50</CY></Size>
            <Text>Sample title</Text><Font>3</Font>
            <TextColor><R>10</R><G>20</G><B>30</B></TextColor>
            <TooltipReference>Your Current Target</TooltipReference>
            <DrawTemplate>WDT_Plain</DrawTemplate>
            <Style_Titlebar>true</Style_Titlebar><Style_Border>true</Style_Border>
            <Style_Closebox>true</Style_Closebox>
            <Pieces>Health</Pieces><Pieces>Percent</Pieces><Pieces>BoxPicture</Pieces>
            <Pieces>Gem0</Pieces><Pieces>Book</Pieces>
            <Pieces>Undefined</Pieces>
        </Screen>
    </XML>"#;

    #[test]
    fn a_stretching_label_keeps_its_anchors() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let window = r#"<XML>
            <Label item="Name"><EQType>134</EQType><AutoStretch>true</AutoStretch>
                <LeftAnchorOffset>1</LeftAnchorOffset><TopAnchorOffset>2</TopAnchorOffset>
                <RightAnchorOffset>-2</RightAnchorOffset><BottomAnchorOffset>14</BottomAnchorOffset>
                <RightAnchorToLeft>false</RightAnchorToLeft>
            </Label>
            <Screen item="Casting"><Size><CX>140</CX><CY>60</CY></Size><Pieces>Name</Pieces></Screen>
        </XML>"#;
        let screen = library.screen(window, "Casting").unwrap();
        let Element::Label(label) = &screen.pieces[0].1 else {
            panic!("a label");
        };
        let area = label.anchors.unwrap().within(100.0, 40.0);
        assert_eq!(
            (area.x, area.y, area.width, area.height),
            (1.0, 2.0, 101.0, 12.0)
        );
    }

    #[test]
    fn a_window_the_skin_makes_transparent_says_so() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let window = r#"<XML>
            <Screen item="Clear"><Size><CX>10</CX><CY>10</CY></Size>
                <Style_Transparent>true</Style_Transparent></Screen>
            <Screen item="Solid"><Size><CX>10</CX><CY>10</CY></Size></Screen>
        </XML>"#;
        assert!(library.screen(window, "Clear").unwrap().transparent);
        assert!(!library.screen(window, "Solid").unwrap().transparent);
    }

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
        assert!(screen.title_bar.is_some() && screen.border);
        assert_eq!(screen.title.as_deref(), Some("Sample title"));
        assert_eq!(
            (screen.title_color, screen.font),
            (Some([10, 20, 30]), Some(3))
        );
        assert_eq!(screen.tooltip.as_deref(), Some("Your Current Target"));
        // A window without words of its own has no title.
        assert_eq!(library.screen(WINDOW, "Bags").unwrap().title, None);
        let template = screen.template.unwrap();
        assert_eq!(template.background.as_deref(), Some("rock.tga"));
        assert_eq!(template.border.top_left.unwrap().texture, "frame.tga");
        // A piece the library lacks is simply not drawn.
        assert!(template.border.top.is_none());
        assert_eq!(template.title[1].as_ref().unwrap().y, 7);
        // Its title bar has a close box, in the template's look, and no
        // minimize box.
        assert_eq!(
            screen.title_bar,
            Some(TitleBar {
                close_box: true,
                minimize_box: false,
            })
        );
        let close = template.close_box.as_ref().unwrap();
        assert!(close.normal.is_some() && close.pressed.is_some());
        assert_eq!(template.minimize_box, None);
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
        // Without its own offset, the bar sits 16 down, as the schema says.
        assert_eq!(
            (gauge.text_offset, gauge.bar_offset, gauge.bar_left),
            ((8.0, 0.0), 16.0, 20.0)
        );
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
    fn a_stretched_button_hugs_the_edges_it_is_anchored_to() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let screen = library.screen(WINDOW, "Bags").unwrap();
        let Element::Button(button) = &screen.pieces[2].1 else {
            panic!("a button")
        };
        // Left and right count from the left; top and bottom from the bottom.
        assert_eq!(
            button.anchors.unwrap().within(226.0, 330.0),
            Area {
                x: 5.0,
                y: 293.0,
                width: 85.0,
                height: 32.0
            }
        );
        let Element::Button(platinum) = &screen.pieces[1].1 else {
            panic!("a button")
        };
        assert_eq!(platinum.anchors, None);
    }

    #[test]
    fn a_setting_given_twice_counts_as_the_first_and_a_window_within_scrolls() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let text = r#"<XML>
            <Screen item="Slots"><Style_VScroll>true</Style_VScroll>
                <DrawTemplate>WDT_Plain</DrawTemplate></Screen>
            <Screen item="LootWnd"><Size><CX>120</CX><CY>420</CY></Size>
                <Style_Titlebar>true</Style_Titlebar><Style_Titlebar>false</Style_Titlebar>
                <Pieces>Slots</Pieces></Screen>
        </XML>"#;
        let screen = library.screen(text, "LootWnd").unwrap();
        assert!(screen.title_bar.is_some());
        let Element::View(view) = &screen.pieces[0].1 else {
            panic!("a view")
        };
        assert!(
            view.scrollbar
                .as_ref()
                .is_some_and(|bar| bar.up.normal.is_some())
        );
    }

    #[test]
    fn a_window_within_a_window_stretches_with_its_anchors() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let text = r#"<XML>
            <Button item="Slot0"><ScreenID>PetBuff0</ScreenID>
                <Location><X>3</X><Y>3</Y></Location><Size><CX>24</CX><CY>24</CY></Size></Button>
            <Screen item="Buffs"><AutoStretch>true</AutoStretch>
                <LeftAnchorOffset>112</LeftAnchorOffset><TopAnchorOffset>4</TopAnchorOffset>
                <RightAnchorOffset>4</RightAnchorOffset><BottomAnchorOffset>4</BottomAnchorOffset>
                <TopAnchorToTop>true</TopAnchorToTop><BottomAnchorToTop>false</BottomAnchorToTop>
                <RightAnchorToLeft>false</RightAnchorToLeft><LeftAnchorToLeft>true</LeftAnchorToLeft>
                <Pieces>Slot0</Pieces></Screen>
            <Screen item="PetInfoWindow"><Size><CX>154</CX><CY>142</CY></Size>
                <Pieces>Buffs</Pieces></Screen>
        </XML>"#;
        let screen = library.screen(text, "PetInfoWindow").unwrap();
        let Element::View(view) = &screen.pieces[0].1 else {
            panic!("a view")
        };
        let Element::Button(slot) = &view.pieces[0].1 else {
            panic!("a button")
        };
        assert_eq!(slot.id.as_deref(), Some("PetBuff0"));
        // Inside a container 146 wide and 120 high, it keeps 112 from the
        // left and 4 from the other edges.
        assert_eq!(
            view.anchors.unwrap().within(146.0, 120.0),
            Area {
                x: 112.0,
                y: 4.0,
                width: 30.0,
                height: 112.0
            }
        );
    }

    #[test]
    fn a_button_that_stretches_keeps_to_its_anchors() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        // The Skills window's Done button: 102 from the right, 22 above the
        // bottom, to 2 from the right and 2 below the bottom.
        let text = r#"<XML>
            <Button item="SKLW_DoneButton"><ScreenID>DoneButton</ScreenID>
                <AutoStretch>true</AutoStretch>
                <LeftAnchorOffset>102</LeftAnchorOffset><TopAnchorOffset>22</TopAnchorOffset>
                <RightAnchorOffset>2</RightAnchorOffset><BottomAnchorOffset>-2</BottomAnchorOffset>
                <TopAnchorToTop>false</TopAnchorToTop><BottomAnchorToTop>false</BottomAnchorToTop>
                <RightAnchorToLeft>false</RightAnchorToLeft><LeftAnchorToLeft>false</LeftAnchorToLeft>
                <Text>Done</Text></Button>
            <Screen item="SkillsWindow"><Size><CX>300</CX><CY>200</CY></Size>
                <Pieces>SKLW_DoneButton</Pieces></Screen>
        </XML>"#;
        let screen = library.screen(text, "SkillsWindow").unwrap();
        let Element::Button(done) = &screen.pieces[0].1 else {
            panic!("a button")
        };
        assert_eq!(
            done.anchors.unwrap().within(290.0, 170.0),
            Area {
                x: 188.0,
                y: 148.0,
                width: 100.0,
                height: 24.0
            }
        );
    }

    #[test]
    fn a_text_box_the_client_fills_keeps_to_its_anchors() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        // The confirmation dialog's question: 2 from three edges, 24 above
        // the bottom, where the buttons sit.
        let text = r#"<XML>
            <STMLbox item="CD_TextOutput"><ScreenID>TextOutput</ScreenID>
                <AutoStretch>true</AutoStretch>
                <LeftAnchorOffset>2</LeftAnchorOffset><TopAnchorOffset>2</TopAnchorOffset>
                <RightAnchorOffset>2</RightAnchorOffset><BottomAnchorOffset>24</BottomAnchorOffset>
                <TopAnchorToTop>true</TopAnchorToTop><BottomAnchorToTop>false</BottomAnchorToTop>
                <LeftAnchorToLeft>true</LeftAnchorToLeft><RightAnchorToLeft>false</RightAnchorToLeft>
            </STMLbox>
            <Screen item="ConfirmationDialogBox"><Size><CX>274</CX><CY>200</CY></Size>
                <Pieces>CD_TextOutput</Pieces></Screen>
        </XML>"#;
        let screen = library.screen(text, "ConfirmationDialogBox").unwrap();
        let Element::TextBox(question) = &screen.pieces[0].1 else {
            panic!("a text box")
        };
        assert_eq!(question.id.as_deref(), Some("TextOutput"));
        assert_eq!(
            question.anchors.unwrap().within(264.0, 170.0),
            Area {
                x: 2.0,
                y: 2.0,
                width: 260.0,
                height: 144.0
            }
        );
    }

    #[test]
    fn pages_hold_slots_and_windows_within_windows() {
        let library = Library::parse(ANIMATIONS, TEMPLATES).unwrap();
        let screen = library.screen(WINDOW, "Bags").unwrap();
        let Element::Tabs(tabs) = &screen.pieces[0].1 else {
            panic!("tabs")
        };
        assert_eq!((tabs.name.as_str(), tabs.area), ("Tabs", None));
        // Its tabs and its page take the frames it names.
        let tab = tabs.tab_frame.as_ref().unwrap();
        assert_eq!(tab.top.as_ref().unwrap().y, 18);
        assert_eq!(tab.left_top.as_ref().unwrap().y, 7);
        assert!(tab.bottom.is_none());
        assert_eq!(
            tabs.page_frame.as_ref().unwrap().bottom.as_ref().unwrap().y,
            7
        );
        let page = &tabs.pages[0];
        assert_eq!(page.title.as_deref(), Some("Inventory"));
        assert_eq!(page.title_colors, [None, Some([255, 255, 0])]);
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
                    Element::Tabs(tabs) => tabs
                        .pages
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

    /// A login set of four files: the index's order puts the pictures and
    /// templates first, a file that is not well-formed next, then the
    /// screen's own file, and last the file that defines the background the
    /// screen lists.
    fn login_set() -> InterfaceSet {
        let shared = r#"<XML>
            <StaticAnimation item="BG_One">
                <Location><X>0</X><Y>0</Y></Location><Size><CX>256</CX><CY>256</CY></Size>
                <Animation>A_Back</Animation>
            </StaticAnimation>
            <Label item="Words"><Location><X>9</X><Y>9</Y></Location>
                <Size><CX>50</CX><CY>15</CY></Size><Text>Elsewhere</Text></Label>
        </XML>"#;
        let own = r#"<XML>
            <Label item="Words"><Location><X>1</X><Y>2</Y></Location>
                <Size><CX>50</CX><CY>15</CY></Size><Text>Own</Text><NoWrap>false</NoWrap>
            </Label>
            <Button item="Go"><ScreenID>GoButton</ScreenID><Font>4</Font>
                <Location><X>10</X><Y>20</Y></Location><Size><CX>100</CX><CY>31</CY></Size>
                <ButtonDrawTemplate><Normal>A_Fill</Normal></ButtonDrawTemplate></Button>
            <Editbox item="Name"><ScreenID>NameEdit</ScreenID><Font>4</Font>
                <DrawTemplate>WDT_Plain</DrawTemplate><Style_Border>true</Style_Border>
                <Location><X>10</X><Y>60</Y></Location><Size><CX>100</CX><CY>38</CY></Size>
            </Editbox>
            <Listbox item="List"><ScreenID>Rows</ScreenID><Font>3</Font>
                <Location><X>10</X><Y>100</Y></Location><Size><CX>100</CX><CY>80</CY></Size>
                <Columns><Width>60</Width><Heading>One</Heading></Columns></Listbox>
            <Screen item="front"><Size><CX>640</CX><CY>480</CY></Size>
                <Pieces>BG_One</Pieces><Pieces>Words</Pieces><Pieces>Go</Pieces>
                <Pieces>Name</Pieces><Pieces>List</Pieces><Pieces>Nowhere</Pieces>
            </Screen>
        </XML>"#;
        InterfaceSet::parse(vec![
            ("Pictures.xml".into(), ANIMATIONS.into()),
            ("Templates.xml".into(), TEMPLATES.into()),
            ("Broken.xml".into(), "<XML><Label item=\"x\"></XML>".into()),
            ("Front.xml".into(), own.into()),
            ("Shared.xml".into(), shared.into()),
        ])
    }

    #[test]
    fn a_screen_of_a_set_finds_its_pieces_in_the_sets_other_files() {
        let set = login_set();
        let screen = set.screen("front.XML", "front").unwrap();
        let names: Vec<_> = screen
            .pieces
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        // A piece no file defines is left out, as a window's always was.
        assert_eq!(names, ["BG_One", "Words", "Go", "Name", "List"]);
        let Element::Image { area, piece, .. } = &screen.pieces[0].1 else {
            panic!("a picture")
        };
        assert_eq!((area.width, piece.texture.as_str()), (256.0, "pieces.tga"));
        // The screen's own file wins over another's element of the same name.
        let Element::Label(words) = &screen.pieces[1].1 else {
            panic!("a label")
        };
        assert_eq!((words.text.as_str(), words.wraps), ("Own", true));
        let fonts: Vec<_> = screen.pieces[2..]
            .iter()
            .map(|(_, element)| match element {
                Element::Button(button) => button.font,
                Element::TextBox(text) => text.font,
                Element::Listbox(list) => list.font,
                _ => None,
            })
            .collect();
        assert_eq!(fonts, [Some(4), Some(4), Some(3)]);
        let Element::TextBox(name) = &screen.pieces[3].1 else {
            panic!("a box")
        };
        assert!(
            name.template.is_some(),
            "the set's templates draw its boxes"
        );
        assert!(matches!(
            set.screen("Broken.xml", "x"),
            Err(UiLayoutError::MissingFile(file)) if file == "Broken.xml"
        ));
        assert!(matches!(
            set.screen("Front.xml", "Words"),
            Err(UiLayoutError::MissingWindow(name)) if name == "Words"
        ));
    }

    #[test]
    fn a_set_is_read_from_the_files_its_index_names() {
        let install =
            std::env::temp_dir().join(format!("eq-client-sidl-set-{}", std::process::id()));
        let default = install.join("uifiles").join("default");
        let painted = install.join("uifiles").join("painted");
        std::fs::create_dir_all(&default).unwrap();
        std::fs::create_dir_all(&painted).unwrap();
        std::fs::write(
            default.join(LOGIN_SET),
            "<XML><Composite><Include>Pictures.xml</Include><Include>Front.xml</Include>\
             <Include>Absent.xml</Include><Include>../Outside.xml</Include></Composite></XML>",
        )
        .unwrap();
        std::fs::write(default.join("Pictures.xml"), ANIMATIONS).unwrap();
        let front = |words: &str| {
            format!(
                "<XML><Label item=\"Words\"><Size><CX>9</CX><CY>9</CY></Size>\
                 <Text>{words}</Text></Label>\
                 <Screen item=\"front\"><Pieces>Words</Pieces></Screen></XML>"
            )
        };
        std::fs::write(default.join("Front.xml"), front("Default")).unwrap();
        std::fs::write(painted.join("Front.xml"), front("Painted")).unwrap();
        let read = |skin: &str| {
            let set = InterfaceSet::read(&install, skin, LOGIN_SET).unwrap();
            let screen = set.screen("Front.xml", "front").unwrap();
            let Element::Label(words) = &screen.pieces[0].1 else {
                panic!("a label")
            };
            (set.files.len(), words.text.clone(), words.wraps)
        };
        let (default_read, painted_read) = (read("default"), read("painted"));
        std::fs::remove_dir_all(&install).unwrap();
        // The skin's own file wins; a file neither skin has, or that would
        // leave the skin's folder, is left out. A label that says nothing of
        // wrapping keeps to one line.
        assert_eq!(default_read, (2, "Default".to_owned(), false));
        assert_eq!(painted_read, (2, "Painted".to_owned(), false));
    }

    #[test]
    #[ignore = "requires EQ_PROBE_INSTALL, a user-owned client installation"]
    fn the_installed_login_set_defines_the_login_and_server_screens() {
        let install = std::env::var("EQ_PROBE_INSTALL").unwrap();
        let set = InterfaceSet::read(Path::new(&install), "default", LOGIN_SET).unwrap();
        for (file, name) in [
            ("EQLSUI_ConnectWnd.xml", "connect"),
            ("EQLSUI_ServerSelectWnd.xml", "serverselect"),
        ] {
            let screen = set.screen(file, name).unwrap();
            // Names, kinds and places only: never the skin's words.
            println!("{name}: {:?}", screen.area);
            for (piece, element) in &screen.pieces {
                let (kind, area) = match element {
                    Element::Image { area, .. } => ("picture", Some(*area)),
                    Element::Label(label) => ("label", Some(label.area)),
                    Element::Button(button) => ("button", Some(button.area)),
                    Element::TextBox(text) => ("box", Some(text.area)),
                    Element::Listbox(list) => ("list", list.area),
                    Element::View(view) => ("view", Some(view.area)),
                    _ => ("other", None),
                };
                println!("  {piece}: {kind} {area:?}");
            }
            let pictures = screen
                .pieces
                .iter()
                .filter(|(_, element)| matches!(element, Element::Image { .. }))
                .count();
            assert!(pictures > 0, "{name} draws its background");
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
