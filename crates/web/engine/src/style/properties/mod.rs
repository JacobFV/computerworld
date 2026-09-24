//! The property table: one source of truth listing every longhand with its name,
//! value parser, inherited flag and how it computes into a `ComputedStyle` field.
//! `parse.rs` holds the value grammars, `apply.rs` the specified-to-computed step.
//! Shorthands live in `../shorthands.rs` and expand into these longhands.

pub mod apply;
pub mod parse;

use super::computed::*;
use super::values::*;
use crate::css::token::{ComponentValue, Number};
use crate::geom::Au;

/// A parsed (specified) value of a longhand, before computation. One variant per
/// value type; the property table pairs each longhand with the variant its parser
/// produces and its applier consumes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Specified {
    CssWide(CssWide),
    /// A value containing `var()`, substituted and re-parsed at computed-value time.
    /// `property` is the declared name (a longhand or the shorthand it came from).
    Pending {
        property: String,
        value: Vec<ComponentValue>,
    },

    Display(Display),
    Position(Position),
    Float(Float),
    Clear(Clear),
    Visibility(Visibility),
    BoxSizing(BoxSizing),
    Overflow(Overflow),
    ZIndex(ZIndex),
    Direction(Direction),
    Integer(i32),
    /// A `<number>` in millionths.
    Number(Number),

    Sizing(SizingSpec),
    Lpa(LpaSpec),
    Lp(LpSpec),
    /// Two length-percentages (radius corners, transform-origin, border-spacing).
    LpPair(LpSpec, LpSpec),
    /// `normal` or a length (letter-spacing, word-spacing).
    LengthOrNormal(Option<LpSpec>),
    BorderWidth(BorderWidthSpec),
    BorderStyle(BorderStyle),
    Color(ColorSpec),

    FontFamily(Vec<String>),
    FontSize(FontSizeSpec),
    FontWeight(FontWeightSpec),
    FontStyle(FontStyle),
    Bool(bool),
    LineHeight(LineHeightSpec),
    TextAlign(TextAlign),
    TextTransform(TextTransform),
    TextDecorationLine {
        underline: bool,
        overline: bool,
        line_through: bool,
    },
    TextDecorationStyle(TextDecorationStyle),
    TextOverflow(TextOverflow),
    WhiteSpace(WhiteSpace),
    WordBreak(WordBreak),
    OverflowWrap(OverflowWrap),
    ScrollbarWidth(ScrollbarWidth),
    VerticalAlign(VerticalAlignSpec),
    Shadows(Vec<ShadowSpec>),

    ListStyleType(ListStyleType),
    ListStylePosition(ListStylePosition),
    TableLayout(TableLayout),
    BorderCollapse(BorderCollapse),
    CaptionSide(CaptionSide),
    EmptyCells(EmptyCells),

    Images(Vec<ImageSpec>),
    Repeats(Vec<BackgroundRepeat>),
    BgSizes(Vec<BgSizeSpec>),
    /// One axis of `background-position`, per layer.
    LpList(Vec<LpSpec>),
    BgBoxes(Vec<BackgroundBox>),
    Bools(Vec<bool>),
    Transform(Vec<TransformSpec>),

    FlexDirection(FlexDirection),
    FlexWrap(FlexWrap),
    JustifyContent(JustifyContent),
    AlignItems(AlignItems),
    AlignSelf(AlignSelf),
    AlignContent(AlignContent),

    TrackList(TrackListSpec),
    GridAreas(Vec<Vec<String>>),
    AutoTracks(Vec<TrackSizeSpec>),
    GridAutoFlow(GridAutoFlow),
    GridLine(GridLine),

    Cursor(Cursor),
    PointerEvents(PointerEvents),
    UserSelect(UserSelect),
    Appearance(Appearance),
    ObjectFit(ObjectFit),
    AspectRatio(AspectRatio),
    LineClamp(Option<u32>),
    BoxOrientVertical(bool),
    Content(ContentSpec),
    Quotes(Option<Vec<(String, String)>>),
    Counters(Vec<(String, i32)>),

    Idents(Vec<String>),
    Times(Vec<i32>),
    Timings(Vec<TimingFunction>),
    IterationCounts(Vec<Option<i32>>),
    AnimationDirections(Vec<AnimationDirection>),
    AnimationFillModes(Vec<AnimationFillMode>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SizingSpec {
    Auto,
    None,
    Lp(LpSpec),
    MinContent,
    MaxContent,
    FitContent,
    /// `flex-basis: content`.
    Content,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LpaSpec {
    Auto,
    Lp(LpSpec),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BorderWidthSpec {
    Thin,
    Medium,
    Thick,
    Length(LpSpec),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FontSizeSpec {
    /// Index into the absolute-size keyword table (0 = xx-small .. 7 = xxx-large).
    Absolute(u8),
    Larger,
    Smaller,
    Lp(LpSpec),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontWeightSpec {
    Absolute(u16),
    Bolder,
    Lighter,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LineHeightSpec {
    Normal,
    Number(Number),
    Lp(LpSpec),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum VerticalAlignSpec {
    Keyword(VerticalAlign),
    Lp(LpSpec),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShadowSpec {
    pub x: LpSpec,
    pub y: LpSpec,
    pub blur: LpSpec,
    pub spread: LpSpec,
    pub color: Option<ColorSpec>,
    pub inset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BgSizeSpec {
    Auto,
    Cover,
    Contain,
    Explicit(LpaSpec, LpaSpec),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TransformSpec {
    Translate(LpSpec, LpSpec),
    /// Factors in millionths.
    Scale(Number, Number),
    /// Centi-degrees.
    Rotate(i32),
    SkewX(i32),
    SkewY(i32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TrackBreadthSpec {
    Lp(LpSpec),
    /// `fr` in millionths.
    Flex(Number),
    Auto,
    MinContent,
    MaxContent,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TrackSizeSpec {
    Breadth(TrackBreadthSpec),
    MinMax(TrackBreadthSpec, TrackBreadthSpec),
    FitContent(LpSpec),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RepeatCount {
    Fixed(u32),
    AutoFill,
    AutoFit,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TrackEntry {
    LineNames(Vec<String>),
    Track(TrackSizeSpec),
    Repeat(RepeatCount, Vec<TrackEntry>),
}

/// `grid-template-rows/columns`: `none` is the empty list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TrackListSpec {
    pub entries: Vec<TrackEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ContentSpec {
    Normal,
    None,
    Items(Vec<ContentItem>),
}

/// The context a specified value computes in.
pub struct ComputeCtx<'a> {
    pub parent: &'a ComputedStyle,
    /// Relative units against the element's own font (set once `font-size` is applied).
    pub lengths: LengthContext,
    /// Relative units against the parent's font, for `font-size` itself.
    pub parent_lengths: LengthContext,
    pub quirks: bool,
    /// The device's installed fonts, for `font-family`.
    pub fonts: crate::css::FontEnvironment,
    /// Families the page's `@font-face` rules download.
    pub web_fonts: &'a [String],
}

/// One longhand's row in the table.
pub struct PropertyDef {
    pub id: LonghandId,
    pub name: &'static str,
    pub inherited: bool,
    /// Apply phase: 0 font properties, 1 line-height and colour, 2 everything else.
    pub phase: u8,
    pub parse: fn(&mut Parser) -> Option<Specified>,
    /// Returns `false` when the value is invalid at computed-value time.
    pub apply: fn(&mut ComputedStyle, &Specified, &ComputeCtx) -> bool,
    /// Copies this property's computed value from `src` to `dst` (initial/inherit).
    pub copy: fn(&mut ComputedStyle, &ComputedStyle),
}

macro_rules! longhands {
    ($( $id:ident, $name:literal, $inh:expr, $phase:expr, $parse:expr, $apply:expr, |$d:ident, $s:ident| $copy:expr; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        pub enum LonghandId { $( $id, )* }

        pub const LONGHANDS: &[PropertyDef] = &[ $(
            PropertyDef {
                id: LonghandId::$id,
                name: $name,
                inherited: $inh,
                phase: $phase,
                parse: $parse,
                apply: $apply,
                copy: |$d: &mut ComputedStyle, $s: &ComputedStyle| { let _ = $copy; },
            },
        )* ];
    };
}

use apply as a;
use parse as p;

longhands! {
    // Phase 0: the font.
    FontFamily, "font-family", true, 0, p::font_family, a::font_family, |d, s| { d.font.family = s.font.family.clone(); d.font.typeface = s.font.typeface; };
    FontSize, "font-size", true, 0, p::font_size, a::font_size, |d, s| { d.font.size = s.font.size; d.font_size_keyword = s.font_size_keyword; };
    FontWeight, "font-weight", true, 0, p::font_weight, a::font_weight, |d, s| d.font.weight = s.font.weight;
    FontStyle, "font-style", true, 0, p::font_style, a::font_style, |d, s| d.font.style = s.font.style;
    FontVariant, "font-variant", true, 0, p::font_variant, a::font_variant, |d, s| d.font.small_caps = s.font.small_caps;
    // Phase 1: what other lengths and colours depend on.
    LineHeight, "line-height", true, 1, p::line_height, a::line_height, |d, s| d.line_height = s.line_height;
    Color, "color", true, 1, p::color, a::color, |d, s| d.color = s.color;
    // Phase 2: box generation.
    Display, "display", false, 2, p::display, a::display, |d, s| d.display = s.display;
    Position, "position", false, 2, p::position, a::position, |d, s| d.position = s.position;
    Float, "float", false, 2, p::float, a::float, |d, s| d.float = s.float;
    Clear, "clear", false, 2, p::clear, a::clear, |d, s| d.clear = s.clear;
    Visibility, "visibility", true, 2, p::visibility, a::visibility, |d, s| d.visibility = s.visibility;
    BoxSizing, "box-sizing", false, 2, p::box_sizing, a::box_sizing, |d, s| d.box_sizing = s.box_sizing;
    OverflowX, "overflow-x", false, 2, p::overflow, a::overflow_x, |d, s| d.overflow_x = s.overflow_x;
    OverflowY, "overflow-y", false, 2, p::overflow, a::overflow_y, |d, s| d.overflow_y = s.overflow_y;
    ZIndex, "z-index", false, 2, p::z_index, a::z_index, |d, s| d.z_index = s.z_index;
    Direction, "direction", true, 2, p::direction, a::direction, |d, s| d.direction = s.direction;
    // Box model.
    Width, "width", false, 2, p::sizing, a::width, |d, s| d.width = s.width;
    Height, "height", false, 2, p::sizing, a::height, |d, s| d.height = s.height;
    MinWidth, "min-width", false, 2, p::sizing, a::min_width, |d, s| d.min_width = s.min_width;
    MinHeight, "min-height", false, 2, p::sizing, a::min_height, |d, s| d.min_height = s.min_height;
    MaxWidth, "max-width", false, 2, p::max_sizing, a::max_width, |d, s| d.max_width = s.max_width;
    MaxHeight, "max-height", false, 2, p::max_sizing, a::max_height, |d, s| d.max_height = s.max_height;
    MarginTop, "margin-top", false, 2, p::lpa, a::margin_top, |d, s| d.margin.top = s.margin.top;
    MarginRight, "margin-right", false, 2, p::lpa, a::margin_right, |d, s| d.margin.right = s.margin.right;
    MarginBottom, "margin-bottom", false, 2, p::lpa, a::margin_bottom, |d, s| d.margin.bottom = s.margin.bottom;
    MarginLeft, "margin-left", false, 2, p::lpa, a::margin_left, |d, s| d.margin.left = s.margin.left;
    PaddingTop, "padding-top", false, 2, p::lp_non_negative, a::padding_top, |d, s| d.padding.top = s.padding.top;
    PaddingRight, "padding-right", false, 2, p::lp_non_negative, a::padding_right, |d, s| d.padding.right = s.padding.right;
    PaddingBottom, "padding-bottom", false, 2, p::lp_non_negative, a::padding_bottom, |d, s| d.padding.bottom = s.padding.bottom;
    PaddingLeft, "padding-left", false, 2, p::lp_non_negative, a::padding_left, |d, s| d.padding.left = s.padding.left;
    BorderTopWidth, "border-top-width", false, 2, p::border_width, a::border_top_width, |d, s| d.border.top.width = s.border.top.width;
    BorderRightWidth, "border-right-width", false, 2, p::border_width, a::border_right_width, |d, s| d.border.right.width = s.border.right.width;
    BorderBottomWidth, "border-bottom-width", false, 2, p::border_width, a::border_bottom_width, |d, s| d.border.bottom.width = s.border.bottom.width;
    BorderLeftWidth, "border-left-width", false, 2, p::border_width, a::border_left_width, |d, s| d.border.left.width = s.border.left.width;
    BorderTopStyle, "border-top-style", false, 2, p::border_style, a::border_top_style, |d, s| d.border.top.style = s.border.top.style;
    BorderRightStyle, "border-right-style", false, 2, p::border_style, a::border_right_style, |d, s| d.border.right.style = s.border.right.style;
    BorderBottomStyle, "border-bottom-style", false, 2, p::border_style, a::border_bottom_style, |d, s| d.border.bottom.style = s.border.bottom.style;
    BorderLeftStyle, "border-left-style", false, 2, p::border_style, a::border_left_style, |d, s| d.border.left.style = s.border.left.style;
    BorderTopColor, "border-top-color", false, 2, p::color, a::border_top_color, |d, s| d.border.top.color = s.border.top.color;
    BorderRightColor, "border-right-color", false, 2, p::color, a::border_right_color, |d, s| d.border.right.color = s.border.right.color;
    BorderBottomColor, "border-bottom-color", false, 2, p::color, a::border_bottom_color, |d, s| d.border.bottom.color = s.border.bottom.color;
    BorderLeftColor, "border-left-color", false, 2, p::color, a::border_left_color, |d, s| d.border.left.color = s.border.left.color;
    BorderTopLeftRadius, "border-top-left-radius", false, 2, p::radius, a::border_top_left_radius, |d, s| d.border_radius.top_left = s.border_radius.top_left;
    BorderTopRightRadius, "border-top-right-radius", false, 2, p::radius, a::border_top_right_radius, |d, s| d.border_radius.top_right = s.border_radius.top_right;
    BorderBottomRightRadius, "border-bottom-right-radius", false, 2, p::radius, a::border_bottom_right_radius, |d, s| d.border_radius.bottom_right = s.border_radius.bottom_right;
    BorderBottomLeftRadius, "border-bottom-left-radius", false, 2, p::radius, a::border_bottom_left_radius, |d, s| d.border_radius.bottom_left = s.border_radius.bottom_left;
    Top, "top", false, 2, p::lpa, a::top, |d, s| d.inset.top = s.inset.top;
    Right, "right", false, 2, p::lpa, a::right, |d, s| d.inset.right = s.inset.right;
    Bottom, "bottom", false, 2, p::lpa, a::bottom, |d, s| d.inset.bottom = s.inset.bottom;
    Left, "left", false, 2, p::lpa, a::left, |d, s| d.inset.left = s.inset.left;
    // Text.
    TextAlign, "text-align", true, 2, p::text_align, a::text_align, |d, s| d.text_align = s.text_align;
    TextIndent, "text-indent", true, 2, p::lp, a::text_indent, |d, s| d.text_indent = s.text_indent;
    TextTransform, "text-transform", true, 2, p::text_transform, a::text_transform, |d, s| d.text_transform = s.text_transform;
    TextDecorationLine, "text-decoration-line", false, 2, p::text_decoration_line, a::text_decoration_line, |d, s| { d.text_decoration.underline = s.text_decoration.underline; d.text_decoration.overline = s.text_decoration.overline; d.text_decoration.line_through = s.text_decoration.line_through; };
    TextDecorationColor, "text-decoration-color", false, 2, p::color, a::text_decoration_color, |d, s| d.text_decoration.color = s.text_decoration.color;
    TextDecorationStyle, "text-decoration-style", false, 2, p::text_decoration_style, a::text_decoration_style, |d, s| d.text_decoration.style = s.text_decoration.style;
    TextOverflow, "text-overflow", false, 2, p::text_overflow, a::text_overflow, |d, s| d.text_overflow = s.text_overflow;
    WhiteSpace, "white-space", true, 2, p::white_space, a::white_space, |d, s| d.white_space = s.white_space;
    WordBreak, "word-break", true, 2, p::word_break, a::word_break, |d, s| d.word_break = s.word_break;
    OverflowWrap, "overflow-wrap", true, 2, p::overflow_wrap, a::overflow_wrap, |d, s| d.overflow_wrap = s.overflow_wrap;
    ScrollbarWidth, "scrollbar-width", false, 2, p::scrollbar_width, a::scrollbar_width, |d, s| d.scrollbar_width = s.scrollbar_width;
    LetterSpacing, "letter-spacing", true, 2, p::length_or_normal, a::letter_spacing, |d, s| d.letter_spacing = s.letter_spacing;
    WordSpacing, "word-spacing", true, 2, p::length_or_normal, a::word_spacing, |d, s| d.word_spacing = s.word_spacing;
    VerticalAlign, "vertical-align", false, 2, p::vertical_align, a::vertical_align, |d, s| d.vertical_align = s.vertical_align;
    TextShadow, "text-shadow", true, 2, p::text_shadow, a::text_shadow, |d, s| d.text_shadow = s.text_shadow.clone();
    TabSize, "tab-size", true, 2, p::tab_size, a::tab_size, |d, s| d.tab_size = s.tab_size;
    // Lists and tables.
    ListStyleType, "list-style-type", true, 2, p::list_style_type, a::list_style_type, |d, s| d.list_style_type = s.list_style_type;
    ListStylePosition, "list-style-position", true, 2, p::list_style_position, a::list_style_position, |d, s| d.list_style_position = s.list_style_position;
    ListStyleImage, "list-style-image", true, 2, p::list_style_image, a::list_style_image, |_d, _s| ();
    TableLayout, "table-layout", false, 2, p::table_layout, a::table_layout, |d, s| d.table_layout = s.table_layout;
    BorderCollapse, "border-collapse", true, 2, p::border_collapse, a::border_collapse, |d, s| d.border_collapse = s.border_collapse;
    BorderSpacing, "border-spacing", true, 2, p::border_spacing, a::border_spacing, |d, s| d.border_spacing = s.border_spacing;
    CaptionSide, "caption-side", true, 2, p::caption_side, a::caption_side, |d, s| d.caption_side = s.caption_side;
    EmptyCells, "empty-cells", true, 2, p::empty_cells, a::empty_cells, |d, s| d.empty_cells = s.empty_cells;
    // Backgrounds and effects.
    BackgroundColor, "background-color", false, 2, p::color, a::background_color, |d, s| d.background_color = s.background_color;
    BackgroundImage, "background-image", false, 2, p::background_image, a::background_image, |d, s| a::copy_layers(d, s, a::LayerField::Image);
    BackgroundRepeat, "background-repeat", false, 2, p::background_repeat, a::background_repeat, |d, s| a::copy_layers(d, s, a::LayerField::Repeat);
    BackgroundSize, "background-size", false, 2, p::background_size, a::background_size, |d, s| a::copy_layers(d, s, a::LayerField::Size);
    BackgroundPositionX, "background-position-x", false, 2, p::background_position_x, a::background_position_x, |d, s| a::copy_layers(d, s, a::LayerField::PositionX);
    BackgroundPositionY, "background-position-y", false, 2, p::background_position_y, a::background_position_y, |d, s| a::copy_layers(d, s, a::LayerField::PositionY);
    BackgroundOrigin, "background-origin", false, 2, p::background_box, a::background_origin, |d, s| a::copy_layers(d, s, a::LayerField::Origin);
    BackgroundClip, "background-clip", false, 2, p::background_clip, a::background_clip, |d, s| a::copy_layers(d, s, a::LayerField::Clip);
    BackgroundAttachment, "background-attachment", false, 2, p::background_attachment, a::background_attachment, |d, s| a::copy_layers(d, s, a::LayerField::Attachment);
    BoxShadow, "box-shadow", false, 2, p::box_shadow, a::box_shadow, |d, s| d.box_shadow = s.box_shadow.clone();
    Opacity, "opacity", false, 2, p::opacity, a::opacity, |d, s| d.opacity = s.opacity;
    BackdropFilter, "backdrop-filter", false, 2, p::backdrop_filter, a::backdrop_filter, |d, s| d.backdrop_blur = s.backdrop_blur;
    Transform, "transform", false, 2, p::transform, a::transform, |d, s| d.transform = s.transform.clone();
    TransformOrigin, "transform-origin", false, 2, p::transform_origin, a::transform_origin, |d, s| d.transform_origin = s.transform_origin;
    OutlineWidth, "outline-width", false, 2, p::border_width, a::outline_width, |d, s| d.outline.width = s.outline.width;
    OutlineStyle, "outline-style", false, 2, p::outline_style, a::outline_style, |d, s| d.outline.style = s.outline.style;
    OutlineColor, "outline-color", false, 2, p::outline_color, a::outline_color, |d, s| d.outline.color = s.outline.color;
    OutlineOffset, "outline-offset", false, 2, p::length, a::outline_offset, |d, s| d.outline_offset = s.outline_offset;
    // Flex.
    FlexDirection, "flex-direction", false, 2, p::flex_direction, a::flex_direction, |d, s| d.flex_direction = s.flex_direction;
    FlexWrap, "flex-wrap", false, 2, p::flex_wrap, a::flex_wrap, |d, s| d.flex_wrap = s.flex_wrap;
    FlexGrow, "flex-grow", false, 2, p::non_negative_number, a::flex_grow, |d, s| d.flex_grow = s.flex_grow;
    FlexShrink, "flex-shrink", false, 2, p::non_negative_number, a::flex_shrink, |d, s| d.flex_shrink = s.flex_shrink;
    FlexBasis, "flex-basis", false, 2, p::flex_basis, a::flex_basis, |d, s| d.flex_basis = s.flex_basis;
    Order, "order", false, 2, p::integer, a::order, |d, s| d.order = s.order;
    JustifyContent, "justify-content", false, 2, p::justify_content, a::justify_content, |d, s| d.justify_content = s.justify_content;
    AlignItems, "align-items", false, 2, p::align_items, a::align_items, |d, s| d.align_items = s.align_items;
    AlignSelf, "align-self", false, 2, p::align_self, a::align_self, |d, s| d.align_self = s.align_self;
    AlignContent, "align-content", false, 2, p::align_content, a::align_content, |d, s| d.align_content = s.align_content;
    RowGap, "row-gap", false, 2, p::gap, a::row_gap, |d, s| d.row_gap = s.row_gap;
    ColumnGap, "column-gap", false, 2, p::gap, a::column_gap, |d, s| d.column_gap = s.column_gap;
    // Grid.
    GridTemplateRows, "grid-template-rows", false, 2, p::track_list, a::grid_template_rows, |d, s| d.grid_template_rows = s.grid_template_rows.clone();
    GridTemplateColumns, "grid-template-columns", false, 2, p::track_list, a::grid_template_columns, |d, s| d.grid_template_columns = s.grid_template_columns.clone();
    GridTemplateAreas, "grid-template-areas", false, 2, p::grid_template_areas, a::grid_template_areas, |d, s| d.grid_template_areas = s.grid_template_areas.clone();
    GridAutoRows, "grid-auto-rows", false, 2, p::auto_tracks, a::grid_auto_rows, |d, s| d.grid_auto_rows = s.grid_auto_rows.clone();
    GridAutoColumns, "grid-auto-columns", false, 2, p::auto_tracks, a::grid_auto_columns, |d, s| d.grid_auto_columns = s.grid_auto_columns.clone();
    GridAutoFlow, "grid-auto-flow", false, 2, p::grid_auto_flow, a::grid_auto_flow, |d, s| d.grid_auto_flow = s.grid_auto_flow;
    GridRowStart, "grid-row-start", false, 2, p::grid_line, a::grid_row_start, |d, s| d.grid_row_start = s.grid_row_start.clone();
    GridRowEnd, "grid-row-end", false, 2, p::grid_line, a::grid_row_end, |d, s| d.grid_row_end = s.grid_row_end.clone();
    GridColumnStart, "grid-column-start", false, 2, p::grid_line, a::grid_column_start, |d, s| d.grid_column_start = s.grid_column_start.clone();
    GridColumnEnd, "grid-column-end", false, 2, p::grid_line, a::grid_column_end, |d, s| d.grid_column_end = s.grid_column_end.clone();
    JustifyItems, "justify-items", false, 2, p::justify_items, a::justify_items, |d, s| d.justify_items = s.justify_items;
    JustifySelf, "justify-self", false, 2, p::align_self, a::justify_self, |d, s| d.justify_self = s.justify_self;
    // Interaction and misc.
    Cursor, "cursor", true, 2, p::cursor, a::cursor, |d, s| d.cursor = s.cursor;
    PointerEvents, "pointer-events", true, 2, p::pointer_events, a::pointer_events, |d, s| d.pointer_events = s.pointer_events;
    UserSelect, "user-select", true, 2, p::user_select, a::user_select, |d, s| d.user_select = s.user_select;
    Appearance, "appearance", false, 2, p::appearance, a::appearance, |d, s| d.appearance = s.appearance;
    ObjectFit, "object-fit", false, 2, p::object_fit, a::object_fit, |d, s| d.object_fit = s.object_fit;
    AspectRatio, "aspect-ratio", false, 2, p::aspect_ratio, a::aspect_ratio, |d, s| d.aspect_ratio = s.aspect_ratio;
    LineClamp, "line-clamp", false, 2, p::line_clamp, a::line_clamp, |d, s| d.line_clamp = s.line_clamp;
    BoxOrient, "-webkit-box-orient", false, 2, p::box_orient, a::box_orient, |d, s| d.box_orient_vertical = s.box_orient_vertical;
    Content, "content", false, 2, p::content, a::content, |d, s| d.content = s.content.clone();
    Quotes, "quotes", true, 2, p::quotes, a::quotes, |d, s| d.quotes = s.quotes.clone();
    CounterReset, "counter-reset", false, 2, p::counter_reset, a::counter_reset, |d, s| d.counter_reset = s.counter_reset.clone();
    CounterIncrement, "counter-increment", false, 2, p::counter_increment, a::counter_increment, |d, s| d.counter_increment = s.counter_increment.clone();
    // Transitions and animations.
    TransitionProperty, "transition-property", false, 2, p::transition_property, a::transition_property, |d, s| d.transitions.property = s.transitions.property.clone();
    TransitionDuration, "transition-duration", false, 2, p::times, a::transition_duration, |d, s| d.transitions.duration = s.transitions.duration.clone();
    TransitionTimingFunction, "transition-timing-function", false, 2, p::timing_functions, a::transition_timing_function, |d, s| d.transitions.timing = s.transitions.timing.clone();
    TransitionDelay, "transition-delay", false, 2, p::times, a::transition_delay, |d, s| d.transitions.delay = s.transitions.delay.clone();
    AnimationName, "animation-name", false, 2, p::animation_name, a::animation_name, |d, s| d.animations.name = s.animations.name.clone();
    AnimationDuration, "animation-duration", false, 2, p::times, a::animation_duration, |d, s| d.animations.duration = s.animations.duration.clone();
    AnimationTimingFunction, "animation-timing-function", false, 2, p::timing_functions, a::animation_timing_function, |d, s| d.animations.timing = s.animations.timing.clone();
    AnimationDelay, "animation-delay", false, 2, p::times, a::animation_delay, |d, s| d.animations.delay = s.animations.delay.clone();
    AnimationIterationCount, "animation-iteration-count", false, 2, p::iteration_counts, a::animation_iteration_count, |d, s| d.animations.iteration_count = s.animations.iteration_count.clone();
    AnimationDirection, "animation-direction", false, 2, p::animation_directions, a::animation_direction, |d, s| d.animations.direction = s.animations.direction.clone();
    AnimationFillMode, "animation-fill-mode", false, 2, p::animation_fill_modes, a::animation_fill_mode, |d, s| d.animations.fill_mode = s.animations.fill_mode.clone();
    AnimationPlayState, "animation-play-state", false, 2, p::animation_play_states, a::animation_play_state, |d, s| d.animations.play_state = s.animations.play_state.clone();
}

impl LonghandId {
    pub fn def(self) -> &'static PropertyDef {
        &LONGHANDS[self as usize]
    }
    pub fn name(self) -> &'static str {
        self.def().name
    }
    pub fn inherited(self) -> bool {
        self.def().inherited
    }
    pub const COUNT: usize = LONGHANDS.len();
    pub fn all() -> impl Iterator<Item = LonghandId> {
        LONGHANDS.iter().map(|d| d.id)
    }
    /// Looks a longhand up by name, after alias and vendor-prefix normalisation.
    pub fn by_name(name: &str) -> Option<LonghandId> {
        let n = normalize_property_name(name);
        LONGHANDS.iter().find(|d| d.name == n).map(|d| d.id)
    }
}

/// Strips `-webkit-`/`-moz-` and maps legacy aliases to the canonical longhand or
/// shorthand name. Unknown names come back unchanged (lower-cased).
pub fn normalize_property_name(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let stripped = lower
        .strip_prefix("-webkit-")
        .or_else(|| lower.strip_prefix("-moz-"))
        .unwrap_or(&lower);
    match stripped {
        "word-wrap" => "overflow-wrap",
        "grid-row-gap" => "row-gap",
        "grid-column-gap" => "column-gap",
        "grid-gap" => "gap",
        "box-orient" => "-webkit-box-orient",
        "box-align" | "box-pack" | "box-flex" => return lower,
        "font-smoothing"
        | "osx-font-smoothing"
        | "tap-highlight-color"
        | "text-size-adjust"
        | "overflow-scrolling" => return lower,
        s => s,
    }
    .to_owned()
}

/// Parses a longhand's value: the CSS-wide keywords first, then the property's own
/// grammar, requiring all input consumed. `None` is an invalid value.
pub fn parse_longhand(id: LonghandId, value: &[ComponentValue]) -> Option<Specified> {
    let mut p = Parser::new(value);
    if let Some(k) = parse_css_wide(&mut p) {
        return Some(Specified::CssWide(k));
    }
    if contains_var(value) {
        return Some(Specified::Pending {
            property: id.name().to_owned(),
            value: value.to_vec(),
        });
    }
    p.parse_entirely(id.def().parse)
}

/// The absolute-size keyword table, in px at `medium` = 16px (and the monospace
/// quirk's 13px scale), from the HTML rendering section.
pub fn absolute_font_size(index: u8, monospace: bool) -> Au {
    const NORMAL: [i32; 8] = [9, 10, 13, 16, 18, 24, 32, 48];
    const MONO: [i32; 8] = [9, 10, 11, 13, 15, 20, 26, 39];
    let t = if monospace { MONO } else { NORMAL };
    Au::from_px_i32(t[index.min(7) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent() {
        for (i, d) in LONGHANDS.iter().enumerate() {
            assert_eq!(d.id as usize, i, "{}", d.name);
            assert_eq!(LonghandId::by_name(d.name), Some(d.id));
            assert_eq!(d.id.name(), d.name);
        }
        assert_eq!(
            LonghandId::by_name("-webkit-box-sizing"),
            Some(LonghandId::BoxSizing)
        );
        assert_eq!(
            LonghandId::by_name("-moz-appearance"),
            Some(LonghandId::Appearance)
        );
        assert_eq!(
            LonghandId::by_name("word-wrap"),
            Some(LonghandId::OverflowWrap)
        );
        assert_eq!(
            LonghandId::by_name("grid-row-gap"),
            Some(LonghandId::RowGap)
        );
        assert_eq!(LonghandId::by_name("DISPLAY"), Some(LonghandId::Display));
        assert_eq!(LonghandId::by_name("margin"), None);
        assert_eq!(LonghandId::by_name("nonsense"), None);
    }

    #[test]
    fn initial_and_inherit_copy_every_field() {
        // Copying every longhand from `initial()` onto an arbitrary style yields `initial()`.
        let mut s = ComputedStyle::initial();
        s.display = Display::Block;
        s.font.size = Au::from_px_i32(40);
        s.background = vec![BackgroundLayer {
            image: BackgroundImage::Url("x".into()),
            repeat: BackgroundRepeat::NoRepeat,
            size: BackgroundSize::Cover,
            position: (LengthPercentage::ZERO, LengthPercentage::ZERO),
            origin: BackgroundBox::PaddingBox,
            clip: BackgroundBox::BorderBox,
            attachment_fixed: false,
        }];
        let init = ComputedStyle::initial();
        for d in LONGHANDS {
            (d.copy)(&mut s, &init);
        }
        assert_eq!(s, init);
    }
}
