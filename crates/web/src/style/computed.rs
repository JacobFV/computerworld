//! The computed style of an element: every property layout and paint read, in its
//! computed form (lengths absolute in `Au`, percentages kept where the spec keeps them,
//! keywords as enums, colours resolved except `currentColor` which is resolved here
//! too). This struct is the contract between `style`, `layout` and `paint`. Add
//! fields; do not remove or rename without telling the other owners.
//!
//! Properties that M1 does not implement are present so M2 needs no struct changes; the
//! cascade sets them to their initial values until their parsers exist.

use crate::geom::Au;
use cw_scene::{Color, Typeface};

/// A length or a percentage, as computed values keep them for properties that resolve
/// against a containing block at layout time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LengthPercentage {
    Length(Au),
    /// In 1/100 of a percent: 50% is 5000.
    Percent(i32),
    /// `calc(length + percentage)`: both parts.
    Calc(Au, i32),
}

impl LengthPercentage {
    pub const ZERO: LengthPercentage = LengthPercentage::Length(Au::ZERO);
    /// Resolves against a base length.
    pub fn resolve(self, base: Au) -> Au {
        match self {
            LengthPercentage::Length(l) => l,
            LengthPercentage::Percent(p) => base.percent_of(p),
            LengthPercentage::Calc(l, p) => l + base.percent_of(p),
        }
    }
    /// Resolves when a base exists, else `None` for percentages (auto behaviour).
    pub fn maybe_resolve(self, base: Option<Au>) -> Option<Au> {
        match (self, base) {
            (LengthPercentage::Length(l), _) => Some(l),
            (_, Some(b)) => Some(self.resolve(b)),
            (_, None) => None,
        }
    }
    pub fn is_zero(self) -> bool {
        matches!(self, LengthPercentage::Length(Au::ZERO) | LengthPercentage::Percent(0))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LengthPercentageAuto {
    #[default]
    Auto,
    Set(LengthPercentage),
}

impl LengthPercentageAuto {
    pub fn is_auto(self) -> bool {
        matches!(self, LengthPercentageAuto::Auto)
    }
    pub fn resolve(self, base: Au) -> Option<Au> {
        match self {
            LengthPercentageAuto::Auto => None,
            LengthPercentageAuto::Set(v) => Some(v.resolve(base)),
        }
    }
}

/// `width`, `height`, `min-*`, `max-*`, `flex-basis`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sizing {
    #[default]
    Auto,
    Set(LengthPercentage),
    MinContent,
    MaxContent,
    FitContent,
    /// `max-width: none`, `max-height: none`.
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Display {
    #[default]
    Inline,
    Block,
    InlineBlock,
    ListItem,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    Table,
    InlineTable,
    TableRowGroup,
    TableHeaderGroup,
    TableFooterGroup,
    TableRow,
    TableCell,
    TableColumnGroup,
    TableColumn,
    TableCaption,
    FlowRoot,
    Contents,
    None,
}

impl Display {
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::Inline | Display::InlineBlock | Display::InlineFlex | Display::InlineGrid | Display::InlineTable)
    }
    pub fn is_none(self) -> bool {
        matches!(self, Display::None)
    }
    /// The block-level counterpart, for blockification (floats, absolutes, flex items).
    pub fn blockify(self) -> Display {
        match self {
            Display::Inline | Display::InlineBlock => Display::Block,
            Display::InlineFlex => Display::Flex,
            Display::InlineGrid => Display::Grid,
            Display::InlineTable => Display::Table,
            d => d,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Position {
    #[default]
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Float {
    #[default]
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Clear {
    #[default]
    None,
    Left,
    Right,
    Both,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Overflow {
    #[default]
    Visible,
    Hidden,
    Clip,
    Scroll,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BoxSizing {
    #[default]
    ContentBox,
    BorderBox,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderStyle {
    #[default]
    None,
    Hidden,
    Solid,
    Dashed,
    Dotted,
    Double,
    Groove,
    Ridge,
    Inset,
    Outset,
}

impl BorderStyle {
    pub fn is_visible(self) -> bool {
        !matches!(self, BorderStyle::None | BorderStyle::Hidden)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BorderSide {
    pub width: Au,
    pub style: BorderStyle,
    pub color: Color,
}

impl Default for BorderSide {
    fn default() -> Self {
        BorderSide { width: Au::ZERO, style: BorderStyle::None, color: Color(0, 0, 0, 255) }
    }
}

impl BorderSide {
    /// The used width: zero unless the style draws.
    pub fn used_width(&self) -> Au {
        if self.style.is_visible() { self.width } else { Au::ZERO }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Sides<T> {
    pub fn uniform(v: T) -> Sides<T> {
        Sides { top: v, right: v, bottom: v, left: v }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Corners<T> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_right: T,
    pub bottom_left: T,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextTransform {
    #[default]
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextDecoration {
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    /// `None` means `currentColor`.
    pub color: Option<Color>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WhiteSpace {
    #[default]
    Normal,
    NoWrap,
    Pre,
    PreWrap,
    PreLine,
    BreakSpaces,
}

impl WhiteSpace {
    pub fn collapses(self) -> bool {
        matches!(self, WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine)
    }
    pub fn wraps(self) -> bool {
        !matches!(self, WhiteSpace::NoWrap | WhiteSpace::Pre)
    }
    pub fn preserves_newlines(self) -> bool {
        !matches!(self, WhiteSpace::Normal | WhiteSpace::NoWrap)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
    BreakWord,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverflowWrap {
    #[default]
    Normal,
    Anywhere,
    BreakWord,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Sub,
    Super,
    TextTop,
    TextBottom,
    Middle,
    Top,
    Bottom,
    Length(LengthPercentage),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineHeight {
    #[default]
    Normal,
    /// Unitless multiplier in 1/1000: 1.5 is 1500.
    Number(i32),
    Length(Au),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    Collapse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListStyleType {
    #[default]
    Disc,
    Circle,
    Square,
    Decimal,
    DecimalLeadingZero,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListStylePosition {
    #[default]
    Outside,
    Inside,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TableLayout {
    #[default]
    Auto,
    Fixed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BorderCollapse {
    #[default]
    Separate,
    Collapse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaptionSide {
    #[default]
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EmptyCells {
    #[default]
    Show,
    Hide,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Cursor {
    #[default]
    Auto,
    Default,
    Pointer,
    Text,
    Move,
    NotAllowed,
    Grab,
    Grabbing,
    Crosshair,
    Wait,
    Progress,
    Help,
    ColResize,
    RowResize,
    NsResize,
    EwResize,
    NeswResize,
    NwseResize,
    None,
}

// Flex and grid (M2; initial values from M1 so the struct is complete).

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FlexDirection {
    #[default]
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FlexWrap {
    #[default]
    NoWrap,
    Wrap,
    WrapReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum JustifyContent {
    #[default]
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Start,
    End,
    Left,
    Right,
    Stretch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AlignItems {
    #[default]
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    Start,
    End,
    SelfStart,
    SelfEnd,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AlignSelf {
    #[default]
    Auto,
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AlignContent {
    #[default]
    Normal,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Stretch,
    Start,
    End,
}

/// A grid track size. Percentages resolve against the grid container.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackSize {
    Fixed(LengthPercentage),
    /// `fr` in 1/1000: `1fr` is 1000.
    Flex(i32),
    Auto,
    MinContent,
    MaxContent,
    MinMax(TrackBreadth, TrackBreadth),
    FitContent(LengthPercentage),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackBreadth {
    Fixed(LengthPercentage),
    Flex(i32),
    Auto,
    MinContent,
    MaxContent,
}

/// `grid-template-rows/columns`: explicit tracks with optional line names, and
/// `repeat()` already expanded except for `auto-fill`/`auto-fit` which layout expands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TrackList {
    pub tracks: Vec<TrackSize>,
    /// Line names for line i (0..=tracks.len()).
    pub line_names: Vec<Vec<String>>,
    pub auto_repeat: Option<AutoRepeat>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AutoRepeat {
    pub fill: bool,
    /// Index in `tracks` where the repeat begins.
    pub at: usize,
    pub tracks: Vec<TrackSize>,
    pub line_names: Vec<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridLine {
    #[default]
    Auto,
    /// Line number (negative counts from the end) with optional name.
    Line(i32, Option<String>),
    Span(u32, Option<String>),
    Name(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GridAutoFlow {
    #[default]
    Row,
    Column,
    RowDense,
    ColumnDense,
}

// Backgrounds, effects (M2).

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GradientStop {
    pub color: Color,
    pub position: Option<LengthPercentage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BackgroundImage {
    None,
    /// Same-origin URL, resolved at fetch time.
    Url(String),
    /// Angle in degrees * 100 (clockwise from up) or a side keyword resolved to one.
    LinearGradient { angle_centi_deg: i32, stops: Vec<GradientStop> },
    RadialGradient { circle: bool, stops: Vec<GradientStop> },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BackgroundRepeat {
    #[default]
    Repeat,
    RepeatX,
    RepeatY,
    NoRepeat,
    Space,
    Round,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BackgroundSize {
    #[default]
    Auto,
    Cover,
    Contain,
    Explicit(LengthPercentageAuto, LengthPercentageAuto),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BackgroundBox {
    #[default]
    PaddingBox,
    BorderBox,
    ContentBox,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BackgroundLayer {
    pub image: BackgroundImage,
    pub repeat: BackgroundRepeat,
    pub size: BackgroundSize,
    pub position: (LengthPercentage, LengthPercentage),
    pub origin: BackgroundBox,
    pub clip: BackgroundBox,
    pub attachment_fixed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoxShadow {
    pub offset_x: Au,
    pub offset_y: Au,
    pub blur: Au,
    pub spread: Au,
    pub color: Color,
    pub inset: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextShadow {
    pub offset_x: Au,
    pub offset_y: Au,
    pub blur: Au,
    pub color: Color,
}

/// A 2-D transform function. Rotation in degrees * 100; scale in 1/1000.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransformOp {
    Translate(LengthPercentage, LengthPercentage),
    Scale(i32, i32),
    Rotate(i32),
    SkewX(i32),
    SkewY(i32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ZIndex {
    #[default]
    Auto,
    Int(i32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PointerEvents {
    #[default]
    Auto,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UserSelect {
    #[default]
    Auto,
    None,
    Text,
    All,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Appearance {
    #[default]
    Auto,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ObjectFit {
    #[default]
    Fill,
    Contain,
    Cover,
    None,
    ScaleDown,
}

/// Generated content for `::before`/`::after`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Content {
    #[default]
    Normal,
    None,
    Items(Vec<ContentItem>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContentItem {
    Text(String),
    Attr(String),
    Counter(String, ListStyleType),
    OpenQuote,
    CloseQuote,
    Url(String),
}

/// The font in computed form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Font {
    /// The bundled face the family list resolved to.
    pub typeface: Typeface,
    /// The original family list, for `getComputedStyle` and inheritance.
    pub family: String,
    /// Absolute size in Au (CSS px * 64).
    pub size: Au,
    /// 100..=900.
    pub weight: u16,
    pub style: FontStyle,
    /// `font-variant: small-caps`.
    pub small_caps: bool,
    /// The language for glyph selection (from `lang` attributes).
    pub lang: cw_scene::Lang,
}

impl Font {
    pub fn is_bold(&self) -> bool {
        self.weight >= 600
    }
    pub fn is_italic(&self) -> bool {
        !matches!(self.style, FontStyle::Normal)
    }
    /// Size in whole px for the text metrics tables (they take u16 px).
    pub fn size_px(&self) -> u16 {
        self.size.to_px_round().clamp(1, u16::MAX as i32) as u16
    }
    pub fn scene_style(&self) -> cw_scene::Style {
        cw_scene::Style::new(self.is_bold(), self.is_italic(), self.lang)
    }
}

/// Everything about one element's style that layout and paint read.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedStyle {
    // Box generation
    pub display: Display,
    pub position: Position,
    pub float: Float,
    pub clear: Clear,
    pub visibility: Visibility,
    pub box_sizing: BoxSizing,
    pub overflow_x: Overflow,
    pub overflow_y: Overflow,
    pub z_index: ZIndex,
    pub direction: Direction,

    // Box model
    pub width: Sizing,
    pub height: Sizing,
    pub min_width: Sizing,
    pub min_height: Sizing,
    pub max_width: Sizing,
    pub max_height: Sizing,
    pub margin: Sides<LengthPercentageAuto>,
    pub padding: Sides<LengthPercentage>,
    pub border: Sides<BorderSide>,
    pub border_radius: Corners<(LengthPercentage, LengthPercentage)>,
    /// `top`, `right`, `bottom`, `left`.
    pub inset: Sides<LengthPercentageAuto>,

    // Text and fonts (inherited)
    pub font: Font,
    pub color: Color,
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub text_indent: LengthPercentage,
    pub text_transform: TextTransform,
    pub text_decoration: TextDecoration,
    pub text_overflow: TextOverflow,
    pub white_space: WhiteSpace,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    pub letter_spacing: Au,
    pub word_spacing: Au,
    pub vertical_align: VerticalAlign,
    pub text_shadow: Vec<TextShadow>,
    pub tab_size: u8,

    // Lists and tables
    pub list_style_type: ListStyleType,
    pub list_style_position: ListStylePosition,
    pub table_layout: TableLayout,
    pub border_collapse: BorderCollapse,
    pub border_spacing: (Au, Au),
    pub caption_side: CaptionSide,
    pub empty_cells: EmptyCells,

    // Backgrounds and effects
    pub background_color: Color,
    pub background: Vec<BackgroundLayer>,
    pub box_shadow: Vec<BoxShadow>,
    /// 0..=255.
    pub opacity: u8,
    pub transform: Vec<TransformOp>,
    pub transform_origin: (LengthPercentage, LengthPercentage),
    pub outline: BorderSide,
    pub outline_offset: Au,

    // Flex
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    pub align_self: AlignSelf,
    pub align_content: AlignContent,
    /// In 1/1000.
    pub flex_grow: i32,
    pub flex_shrink: i32,
    pub flex_basis: Sizing,
    pub order: i32,
    pub row_gap: LengthPercentage,
    pub column_gap: LengthPercentage,

    // Grid
    pub grid_template_rows: TrackList,
    pub grid_template_columns: TrackList,
    pub grid_template_areas: Vec<Vec<String>>,
    pub grid_auto_rows: Vec<TrackSize>,
    pub grid_auto_columns: Vec<TrackSize>,
    pub grid_auto_flow: GridAutoFlow,
    pub grid_row_start: GridLine,
    pub grid_row_end: GridLine,
    pub grid_column_start: GridLine,
    pub grid_column_end: GridLine,
    pub justify_items: AlignItems,
    pub justify_self: AlignSelf,

    // Interaction and misc
    pub cursor: Cursor,
    pub pointer_events: PointerEvents,
    pub user_select: UserSelect,
    pub appearance: Appearance,
    pub object_fit: ObjectFit,
    pub content: Content,
    /// `quotes` pairs.
    pub quotes: Vec<(String, String)>,
    pub counter_reset: Vec<(String, i32)>,
    pub counter_increment: Vec<(String, i32)>,
}

impl ComputedStyle {
    /// The initial value of every property, with the UA's root font (16 px sans).
    pub fn initial() -> ComputedStyle {
        ComputedStyle {
            display: Display::Inline,
            position: Position::Static,
            float: Float::None,
            clear: Clear::None,
            visibility: Visibility::Visible,
            box_sizing: BoxSizing::ContentBox,
            overflow_x: Overflow::Visible,
            overflow_y: Overflow::Visible,
            z_index: ZIndex::Auto,
            direction: Direction::Ltr,
            width: Sizing::Auto,
            height: Sizing::Auto,
            min_width: Sizing::Auto,
            min_height: Sizing::Auto,
            max_width: Sizing::None,
            max_height: Sizing::None,
            margin: Sides::uniform(LengthPercentageAuto::Set(LengthPercentage::ZERO)),
            padding: Sides::uniform(LengthPercentage::ZERO),
            border: Sides::uniform(BorderSide::default()),
            border_radius: Corners::default_radius(),
            inset: Sides::uniform(LengthPercentageAuto::Auto),
            font: Font {
                typeface: Typeface::default(),
                family: String::new(),
                size: Au::from_px_i32(16),
                weight: 400,
                style: FontStyle::Normal,
                small_caps: false,
                lang: cw_scene::Lang::Auto,
            },
            color: Color(0, 0, 0, 255),
            line_height: LineHeight::Normal,
            text_align: TextAlign::Start,
            text_indent: LengthPercentage::ZERO,
            text_transform: TextTransform::None,
            text_decoration: TextDecoration::default(),
            text_overflow: TextOverflow::Clip,
            white_space: WhiteSpace::Normal,
            word_break: WordBreak::Normal,
            overflow_wrap: OverflowWrap::Normal,
            letter_spacing: Au::ZERO,
            word_spacing: Au::ZERO,
            vertical_align: VerticalAlign::Baseline,
            text_shadow: Vec::new(),
            tab_size: 8,
            list_style_type: ListStyleType::Disc,
            list_style_position: ListStylePosition::Outside,
            table_layout: TableLayout::Auto,
            border_collapse: BorderCollapse::Separate,
            border_spacing: (Au::ZERO, Au::ZERO),
            caption_side: CaptionSide::Top,
            empty_cells: EmptyCells::Show,
            background_color: Color(0, 0, 0, 0),
            background: Vec::new(),
            box_shadow: Vec::new(),
            opacity: 255,
            transform: Vec::new(),
            transform_origin: (LengthPercentage::Percent(5000), LengthPercentage::Percent(5000)),
            outline: BorderSide::default(),
            outline_offset: Au::ZERO,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::Stretch,
            align_self: AlignSelf::Auto,
            align_content: AlignContent::Normal,
            flex_grow: 0,
            flex_shrink: 1000,
            flex_basis: Sizing::Auto,
            order: 0,
            row_gap: LengthPercentage::ZERO,
            column_gap: LengthPercentage::ZERO,
            grid_template_rows: TrackList::default(),
            grid_template_columns: TrackList::default(),
            grid_template_areas: Vec::new(),
            grid_auto_rows: vec![TrackSize::Auto],
            grid_auto_columns: vec![TrackSize::Auto],
            grid_auto_flow: GridAutoFlow::Row,
            grid_row_start: GridLine::Auto,
            grid_row_end: GridLine::Auto,
            grid_column_start: GridLine::Auto,
            grid_column_end: GridLine::Auto,
            justify_items: AlignItems::Stretch,
            justify_self: AlignSelf::Auto,
            cursor: Cursor::Auto,
            pointer_events: PointerEvents::Auto,
            user_select: UserSelect::Auto,
            appearance: Appearance::Auto,
            object_fit: ObjectFit::Fill,
            content: Content::Normal,
            quotes: vec![("\u{201C}".into(), "\u{201D}".into()), ("\u{2018}".into(), "\u{2019}".into())],
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
        }
    }

    /// Copies the inherited properties from a parent onto the initial values.
    pub fn inherit_from(parent: &ComputedStyle) -> ComputedStyle {
        let mut s = ComputedStyle::initial();
        s.direction = parent.direction;
        s.visibility = parent.visibility;
        s.font = parent.font.clone();
        s.color = parent.color;
        s.line_height = parent.line_height;
        s.text_align = parent.text_align;
        s.text_indent = parent.text_indent;
        s.text_transform = parent.text_transform;
        s.white_space = parent.white_space;
        s.word_break = parent.word_break;
        s.overflow_wrap = parent.overflow_wrap;
        s.letter_spacing = parent.letter_spacing;
        s.word_spacing = parent.word_spacing;
        s.text_shadow = parent.text_shadow.clone();
        s.tab_size = parent.tab_size;
        s.list_style_type = parent.list_style_type;
        s.list_style_position = parent.list_style_position;
        s.border_collapse = parent.border_collapse;
        s.border_spacing = parent.border_spacing;
        s.caption_side = parent.caption_side;
        s.empty_cells = parent.empty_cells;
        s.cursor = parent.cursor;
        s.pointer_events = parent.pointer_events;
        s.user_select = parent.user_select;
        s.quotes = parent.quotes.clone();
        s
    }

    pub fn is_positioned(&self) -> bool {
        !matches!(self.position, Position::Static)
    }
    pub fn is_floating(&self) -> bool {
        !matches!(self.float, Float::None)
    }
    pub fn is_out_of_flow(&self) -> bool {
        matches!(self.position, Position::Absolute | Position::Fixed) || self.is_floating()
    }
    /// A stacking context is established by these conditions (CSS 2.1 + transforms,
    /// opacity, isolation via flex/grid items with z-index).
    pub fn establishes_stacking_context(&self, is_flex_or_grid_item: bool) -> bool {
        (self.is_positioned() && !matches!(self.z_index, ZIndex::Auto))
            || matches!(self.position, Position::Fixed | Position::Sticky)
            || self.opacity < 255
            || !self.transform.is_empty()
            || (is_flex_or_grid_item && !matches!(self.z_index, ZIndex::Auto))
    }
    pub fn used_border_widths(&self) -> crate::geom::Edges {
        crate::geom::Edges {
            top: self.border.top.used_width(),
            right: self.border.right.used_width(),
            bottom: self.border.bottom.used_width(),
            left: self.border.left.used_width(),
        }
    }
    /// `line-height` resolved to a length, given the font's normal line height.
    pub fn line_height_au(&self, normal: Au) -> Au {
        match self.line_height {
            LineHeight::Normal => normal,
            LineHeight::Number(n) => self.font.size.scale(n, 1000),
            LineHeight::Length(l) => l,
        }
    }
}

impl Corners<(LengthPercentage, LengthPercentage)> {
    pub fn default_radius() -> Self {
        let z = (LengthPercentage::ZERO, LengthPercentage::ZERO);
        Corners { top_left: z, top_right: z, bottom_right: z, bottom_left: z }
    }
    pub fn is_zero(&self) -> bool {
        [self.top_left, self.top_right, self.bottom_right, self.bottom_left].iter().all(|(a, b)| a.is_zero() && b.is_zero())
    }
}

/// Computed styles for a document, keyed by node id; pseudo-elements keyed separately.
#[derive(Clone, Debug, Default)]
pub struct StyleSet {
    styles: Vec<Option<std::rc::Rc<ComputedStyle>>>,
    before: std::collections::BTreeMap<crate::dom::NodeId, std::rc::Rc<ComputedStyle>>,
    after: std::collections::BTreeMap<crate::dom::NodeId, std::rc::Rc<ComputedStyle>>,
    marker: std::collections::BTreeMap<crate::dom::NodeId, std::rc::Rc<ComputedStyle>>,
}

impl StyleSet {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&mut self, id: crate::dom::NodeId, style: std::rc::Rc<ComputedStyle>) {
        if self.styles.len() <= id.index() {
            self.styles.resize(id.index() + 1, None);
        }
        self.styles[id.index()] = Some(style);
    }
    pub fn get(&self, id: crate::dom::NodeId) -> Option<&ComputedStyle> {
        self.styles.get(id.index()).and_then(|s| s.as_deref())
    }
    pub fn get_rc(&self, id: crate::dom::NodeId) -> Option<&std::rc::Rc<ComputedStyle>> {
        self.styles.get(id.index()).and_then(|s| s.as_ref())
    }
    pub fn set_before(&mut self, id: crate::dom::NodeId, style: std::rc::Rc<ComputedStyle>) {
        self.before.insert(id, style);
    }
    pub fn set_after(&mut self, id: crate::dom::NodeId, style: std::rc::Rc<ComputedStyle>) {
        self.after.insert(id, style);
    }
    pub fn set_marker(&mut self, id: crate::dom::NodeId, style: std::rc::Rc<ComputedStyle>) {
        self.marker.insert(id, style);
    }
    pub fn before(&self, id: crate::dom::NodeId) -> Option<&ComputedStyle> {
        self.before.get(&id).map(|s| &**s)
    }
    pub fn after(&self, id: crate::dom::NodeId) -> Option<&ComputedStyle> {
        self.after.get(&id).map(|s| &**s)
    }
    pub fn marker(&self, id: crate::dom::NodeId) -> Option<&ComputedStyle> {
        self.marker.get(&id).map(|s| &**s)
    }
    pub fn clear(&mut self, id: crate::dom::NodeId) {
        if let Some(s) = self.styles.get_mut(id.index()) {
            *s = None;
        }
        self.before.remove(&id);
        self.after.remove(&id);
        self.marker.remove(&id);
    }
}
