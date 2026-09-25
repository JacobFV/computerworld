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
    /// `min()`, `max()` or `clamp()` whose operands mix lengths and percentages, so the
    /// comparison waits for the percentage base: `v` (length, percent) clamped below
    /// by `lo` and above by `hi`; the lower bound wins, as in `clamp()`.
    Clamp {
        lo: Option<(Au, i32)>,
        v: (Au, i32),
        hi: Option<(Au, i32)>,
    },
}

impl LengthPercentage {
    pub const ZERO: LengthPercentage = LengthPercentage::Length(Au::ZERO);
    /// Resolves against a base length.
    pub fn resolve(self, base: Au) -> Au {
        match self {
            LengthPercentage::Length(l) => l,
            LengthPercentage::Percent(p) => base.percent_of(p),
            LengthPercentage::Calc(l, p) => l + base.percent_of(p),
            LengthPercentage::Clamp { lo, v, hi } => {
                let part = |(l, p): (Au, i32)| l + base.percent_of(p);
                let mut r = part(v);
                if let Some(hi) = hi {
                    r = r.min(part(hi));
                }
                if let Some(lo) = lo {
                    r = r.max(part(lo));
                }
                r
            }
        }
    }
    /// True when a percentage takes part, so the value needs a base.
    pub fn has_percent(self) -> bool {
        match self {
            LengthPercentage::Length(_) => false,
            LengthPercentage::Percent(_) => true,
            LengthPercentage::Calc(_, p) => p != 0,
            LengthPercentage::Clamp { lo, v, hi } => {
                v.1 != 0 || lo.is_some_and(|b| b.1 != 0) || hi.is_some_and(|b| b.1 != 0)
            }
        }
    }
    /// Resolves when a base exists, else `None` for percentages (auto behaviour).
    pub fn maybe_resolve(self, base: Option<Au>) -> Option<Au> {
        match (self, base) {
            (LengthPercentage::Length(l), _) => Some(l),
            (_, Some(b)) => Some(self.resolve(b)),
            (v, None) if !v.has_percent() => Some(v.resolve(Au::ZERO)),
            (_, None) => None,
        }
    }
    pub fn is_zero(self) -> bool {
        matches!(
            self,
            LengthPercentage::Length(Au::ZERO) | LengthPercentage::Percent(0)
        )
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
        matches!(
            self,
            Display::Inline
                | Display::InlineBlock
                | Display::InlineFlex
                | Display::InlineGrid
                | Display::InlineTable
        )
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
        BorderSide {
            width: Au::ZERO,
            style: BorderStyle::None,
            color: Color(0, 0, 0, 255),
        }
    }
}

impl BorderSide {
    /// The used width: zero unless the style draws.
    pub fn used_width(&self) -> Au {
        if self.style.is_visible() {
            self.width
        } else {
            Au::ZERO
        }
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
        Sides {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
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
    /// `-webkit-center`: centres the lines and also the block-level children whose
    /// margins are not `auto` (what `<center>` and `align=center` mean in HTML).
    WebkitCenter,
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
    pub style: TextDecorationStyle,
}

impl TextDecoration {
    pub fn any_line(&self) -> bool {
        self.underline || self.overline || self.line_through
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextDecorationStyle {
    #[default]
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
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
        matches!(
            self,
            WhiteSpace::Normal | WhiteSpace::NoWrap | WhiteSpace::PreLine
        )
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

/// `scrollbar-width` (css-scrollbars-1): how much of the scrollport a scroll
/// container gives its bars. `none` reserves nothing and paints nothing, which is
/// how a horizontal chip rail hides the gutter a desktop browser would take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScrollbarWidth {
    #[default]
    Auto,
    Thin,
    None,
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
    LinearGradient {
        angle_centi_deg: i32,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        circle: bool,
        stops: Vec<GradientStop>,
    },
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

/// `aspect-ratio`: `auto || <ratio>`. The ratio is width over height, both positive,
/// in millionths (the css `Number` scale); a degenerate ratio (a zero side) computes
/// to no ratio, as the specification says it behaves as `auto`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AspectRatio {
    /// `auto` was given: a replaced element prefers its natural ratio.
    pub auto: bool,
    pub ratio: Option<(i64, i64)>,
}

impl AspectRatio {
    /// The height that goes with a width, both of the box `box-sizing` names.
    pub fn height_for(self, width: Au) -> Option<Au> {
        let (w, h) = self.ratio?;
        Some(Au(
            (width.0 as i128 * h as i128 / w as i128).clamp(0, Au::MAX.0 as i128) as i32,
        ))
    }
    /// The width that goes with a height.
    pub fn width_for(self, height: Au) -> Option<Au> {
        let (w, h) = self.ratio?;
        Some(Au(
            (height.0 as i128 * w as i128 / h as i128).clamp(0, Au::MAX.0 as i128) as i32,
        ))
    }
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

/// `transition-timing-function` / `animation-timing-function`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TimingFunction {
    #[default]
    Ease,
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    StepStart,
    StepEnd,
    /// Control points in 1/1000.
    CubicBezier(i32, i32, i32, i32),
    /// Steps and whether the jump is at the start.
    Steps(u32, bool),
}

/// The `transition-*` longhands as coordinated lists (CSS Transitions §2). `items()`
/// pairs them up the way the spec does, repeating shorter lists.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TransitionList {
    /// `all`, `none` or a property name per item.
    pub property: Vec<String>,
    /// Milliseconds.
    pub duration: Vec<i32>,
    pub timing: Vec<TimingFunction>,
    pub delay: Vec<i32>,
}

impl Default for TransitionList {
    fn default() -> Self {
        TransitionList {
            property: vec!["all".into()],
            duration: vec![0],
            timing: vec![TimingFunction::Ease],
            delay: vec![0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Transition {
    pub property: String,
    pub duration_ms: i32,
    pub timing: TimingFunction,
    pub delay_ms: i32,
}

impl TransitionList {
    pub fn items(&self) -> Vec<Transition> {
        let n = self.property.len();
        (0..n)
            .map(|i| Transition {
                property: self.property[i].clone(),
                duration_ms: self.duration[i % self.duration.len().max(1)],
                timing: self.timing[i % self.timing.len().max(1)],
                delay_ms: self.delay[i % self.delay.len().max(1)],
            })
            .filter(|t| t.property != "none")
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AnimationDirection {
    #[default]
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AnimationFillMode {
    #[default]
    None,
    Forwards,
    Backwards,
    Both,
}

/// The `animation-*` longhands as coordinated lists.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AnimationList {
    /// `none` or a `@keyframes` name per item.
    pub name: Vec<String>,
    pub duration: Vec<i32>,
    pub timing: Vec<TimingFunction>,
    pub delay: Vec<i32>,
    /// In 1/1000; `None` is `infinite`.
    pub iteration_count: Vec<Option<i32>>,
    pub direction: Vec<AnimationDirection>,
    pub fill_mode: Vec<AnimationFillMode>,
    /// `true` when running.
    pub play_state: Vec<bool>,
}

impl Default for AnimationList {
    fn default() -> Self {
        AnimationList {
            name: vec!["none".into()],
            duration: vec![0],
            timing: vec![TimingFunction::Ease],
            delay: vec![0],
            iteration_count: vec![Some(1000)],
            direction: vec![AnimationDirection::Normal],
            fill_mode: vec![AnimationFillMode::None],
            play_state: vec![true],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Animation {
    pub name: String,
    pub duration_ms: i32,
    pub timing: TimingFunction,
    pub delay_ms: i32,
    pub iteration_count: Option<i32>,
    pub direction: AnimationDirection,
    pub fill_mode: AnimationFillMode,
    pub running: bool,
}

impl AnimationList {
    pub fn items(&self) -> Vec<Animation> {
        let n = self.name.len();
        let at = |i: usize, len: usize| i % len.max(1);
        (0..n)
            .map(|i| Animation {
                name: self.name[i].clone(),
                duration_ms: self.duration[at(i, self.duration.len())],
                timing: self.timing[at(i, self.timing.len())],
                delay_ms: self.delay[at(i, self.delay.len())],
                iteration_count: self.iteration_count[at(i, self.iteration_count.len())],
                direction: self.direction[at(i, self.direction.len())],
                fill_mode: self.fill_mode[at(i, self.fill_mode.len())],
                running: self.play_state[at(i, self.play_state.len())],
            })
            .filter(|a| a.name != "none")
            .collect()
    }
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
    /// The same size unrounded, in millionths of a px (0 when not known), which
    /// glyphs are scaled by: see [`Font::glyph_size`].
    pub size_micro: i64,
    /// 100..=900.
    pub weight: u16,
    pub style: FontStyle,
    /// `font-variant: small-caps`.
    pub small_caps: bool,
    /// `font-kerning: none`.
    pub kerning_none: bool,
    /// What `font-feature-settings` says of the `kern` feature: -1 nothing, 0 off,
    /// 1 on. It overrides `font-kerning`, as the lower-level feature control does.
    pub kern_feature: i8,
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
    /// The size glyphs are scaled at, in `Au`: the unrounded size truncated to 1/64 px,
    /// as FreeType's 26.6 character size truncates Skia's float size in Chromium
    /// (8pt, 10.6667 px, is set at 10.65625 px while `size` rounds to 10.671875).
    /// `size` itself when the unrounded size is unknown or does not round to it
    /// (a size assigned directly).
    pub fn glyph_size(&self) -> Au {
        let fine = self.size_micro * 64;
        if self.size_micro > 0 && (fine - i64::from(self.size.0) * 1_000_000).abs() <= 500_000 {
            Au(fine.div_euclid(1_000_000) as i32)
        } else {
            self.size
        }
    }
    /// Whether pair kerning applies: `font-kerning` unless `font-feature-settings`
    /// names `kern`. (`letter-spacing` does not turn it off: Chromium keeps the
    /// `kern` feature and adds the spacing, see `layout::text::kern_spaced`.)
    pub fn kerns(&self) -> bool {
        match self.kern_feature {
            0 => false,
            1 => true,
            _ => !self.kerning_none,
        }
    }
    /// Size in whole px for the text metrics tables (they take u16 px).
    pub fn size_px(&self) -> u16 {
        self.size.to_px_round().clamp(1, u16::MAX as i32) as u16
    }
    /// The run's style for `cw_scene` measurement and drawing, marked as web content
    /// so it kerns and sets monospace on its real advances, as Chromium does.
    pub fn scene_style(&self) -> cw_scene::Style {
        cw_scene::Style::new(self.is_bold(), self.is_italic(), self.lang).for_web()
    }
}

/// Custom properties by name (with the `--`): value tokens after `var()` substitution.
pub type CustomProperties =
    std::collections::BTreeMap<String, Vec<crate::css::token::ComponentValue>>;

thread_local! {
    static INITIAL: std::rc::Rc<ComputedStyle> = std::rc::Rc::new(ComputedStyle::build_initial());
}

/// Everything about one element's style that layout and paint read.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComputedStyle {
    // Box generation
    pub display: Display,
    /// `display` was inline-level before an absolute position blockified it: the
    /// box's static position is where it would sit in its line, not below it.
    pub inline_origin: bool,
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
    /// When `font-size` is an absolute-size keyword, its index (0 = xx-small), so the
    /// monospace scale quirk can rescale it when the family changes. Inherited.
    pub font_size_keyword: Option<u8>,
    pub color: Color,
    pub line_height: LineHeight,
    pub text_align: TextAlign,
    pub text_indent: LengthPercentage,
    pub text_transform: TextTransform,
    pub text_decoration: TextDecoration,
    /// The decorations to draw on this element's text: its own plus those propagated
    /// from ancestors (CSS 2.1 §16.3.1: not into floats, absolutes or atomic inlines).
    /// Paint reads this; `text_decoration` is the element's own computed value.
    pub text_decoration_effective: TextDecoration,
    pub text_overflow: TextOverflow,
    pub white_space: WhiteSpace,
    pub word_break: WordBreak,
    pub overflow_wrap: OverflowWrap,
    pub scrollbar_width: ScrollbarWidth,
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
    /// `backdrop-filter`'s blur radius; zero for none.
    pub backdrop_blur: Au,

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
    pub aspect_ratio: AspectRatio,
    /// `-webkit-line-clamp`: the number of lines a legacy vertical box shows.
    pub line_clamp: Option<u32>,
    /// `-webkit-box-orient: vertical`.
    pub box_orient_vertical: bool,
    pub content: Content,
    /// `quotes` pairs.
    pub quotes: Vec<(String, String)>,
    pub counter_reset: Vec<(String, i32)>,
    pub counter_increment: Vec<(String, i32)>,

    // Custom properties (inherited): the declared value tokens after `var()`
    // substitution, keyed by the full name including `--`.
    /// Shared: most elements inherit their parent's set unchanged.
    pub custom: std::rc::Rc<CustomProperties>,

    // Transitions and animations (parsed in M1; M2 runs them on the world clock).
    pub transitions: TransitionList,
    pub animations: AnimationList,
}

impl ComputedStyle {
    /// The initial value of every property, with the UA's root font (16 px sans).
    pub fn initial() -> ComputedStyle {
        INITIAL.with(|s| (**s).clone())
    }

    /// [`ComputedStyle::initial`], shared.
    pub fn initial_rc() -> std::rc::Rc<ComputedStyle> {
        INITIAL.with(|s| s.clone())
    }

    fn build_initial() -> ComputedStyle {
        ComputedStyle {
            display: Display::Inline,
            inline_origin: false,
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
                size_micro: 16_000_000,
                weight: 400,
                style: FontStyle::Normal,
                small_caps: false,
                kerning_none: false,
                kern_feature: -1,
                lang: cw_scene::Lang::Auto,
            },
            font_size_keyword: Some(3),
            color: Color(0, 0, 0, 255),
            line_height: LineHeight::Normal,
            text_align: TextAlign::Start,
            text_indent: LengthPercentage::ZERO,
            text_transform: TextTransform::None,
            text_decoration: TextDecoration::default(),
            text_decoration_effective: TextDecoration::default(),
            text_overflow: TextOverflow::Clip,
            white_space: WhiteSpace::Normal,
            word_break: WordBreak::Normal,
            overflow_wrap: OverflowWrap::Normal,
            scrollbar_width: ScrollbarWidth::Auto,
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
            transform_origin: (
                LengthPercentage::Percent(5000),
                LengthPercentage::Percent(5000),
            ),
            outline: BorderSide::default(),
            outline_offset: Au::ZERO,
            backdrop_blur: Au::ZERO,
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
            aspect_ratio: AspectRatio::default(),
            line_clamp: None,
            box_orient_vertical: false,
            content: Content::Normal,
            quotes: vec![
                ("\u{201C}".into(), "\u{201D}".into()),
                ("\u{2018}".into(), "\u{2019}".into()),
            ],
            counter_reset: Vec::new(),
            counter_increment: Vec::new(),
            custom: Default::default(),
            transitions: TransitionList::default(),
            animations: AnimationList::default(),
        }
    }

    /// Copies the inherited properties from a parent onto the initial values.
    pub fn inherit_from(parent: &ComputedStyle) -> ComputedStyle {
        let mut s = ComputedStyle::initial();
        s.direction = parent.direction;
        s.visibility = parent.visibility;
        s.font = parent.font.clone();
        s.font_size_keyword = parent.font_size_keyword;
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
        s.custom = parent.custom.clone();
        s
    }

    /// Whether the two styles lay out the same: equal but for properties only
    /// paint and hit testing read (colours, backgrounds, shadows, outlines, cursor,
    /// `pointer-events`, custom properties, transitions), with opacity compared only
    /// as far as it makes a stacking context.
    pub fn layout_eq(&self, other: &ComputedStyle) -> bool {
        let mut o = other.clone();
        o.color = self.color;
        o.background_color = self.background_color;
        o.background.clone_from(&self.background);
        o.box_shadow.clone_from(&self.box_shadow);
        o.text_shadow.clone_from(&self.text_shadow);
        o.outline = self.outline;
        o.outline_offset = self.outline_offset;
        for (a, b) in [
            (&mut o.border.top, &self.border.top),
            (&mut o.border.right, &self.border.right),
            (&mut o.border.bottom, &self.border.bottom),
            (&mut o.border.left, &self.border.left),
        ] {
            a.color = b.color;
        }
        o.border_radius = self.border_radius;
        o.text_decoration = self.text_decoration;
        o.text_decoration_effective = self.text_decoration_effective;
        o.cursor = self.cursor;
        o.pointer_events = self.pointer_events;
        o.user_select = self.user_select;
        o.custom = self.custom.clone();
        o.transitions.clone_from(&self.transitions);
        o.animations.clone_from(&self.animations);
        if (o.opacity < 255) == (self.opacity < 255) {
            o.opacity = self.opacity;
        }
        *self == o
    }

    /// Whether hit testing sees the two styles the same: they lay out the same
    /// and agree on `pointer-events` and the corner radii (a rounded box's hit area).
    pub fn hit_eq(&self, other: &ComputedStyle) -> bool {
        self.pointer_events == other.pointer_events
            && self.border_radius == other.border_radius
            && self.layout_eq(other)
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
            || self.backdrop_blur > Au::ZERO
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
            // A number multiplies the font size; the product is truncated to 1/64 px
            // (Blink's `LayoutUnit(float)`), so `1.6` on 14 px is 22.390625, not 22.40625.
            LineHeight::Number(n) => {
                Au((self.font.size.0 as i64 * n as i64).div_euclid(1000) as i32)
            }
            LineHeight::Length(l) => l,
        }
    }
}

impl Corners<(LengthPercentage, LengthPercentage)> {
    pub fn default_radius() -> Self {
        let z = (LengthPercentage::ZERO, LengthPercentage::ZERO);
        Corners {
            top_left: z,
            top_right: z,
            bottom_right: z,
            bottom_left: z,
        }
    }
    pub fn is_zero(&self) -> bool {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
        .iter()
        .all(|(a, b)| a.is_zero() && b.is_zero())
    }
}

/// One `@keyframes` rule's frames: selectors (`from`, `to`, percentages) and their
/// declarations, in source order.
pub type Keyframes = Vec<(
    Vec<crate::css::KeyframeSelector>,
    Vec<crate::css::Declaration>,
)>;

/// Computed styles for a document, keyed by node id; pseudo-elements keyed separately.
/// Text nodes share their parent's style. Also carries what the cascade collected for
/// other modules: `@font-face` rules, `@keyframes`, what was unsupported, and the
/// viewport the styles were computed for.
#[derive(Clone, Debug, Default)]
pub struct StyleSet {
    pub(crate) styles: Vec<Option<std::rc::Rc<ComputedStyle>>>,
    pub(crate) before: std::collections::BTreeMap<crate::dom::NodeId, std::rc::Rc<ComputedStyle>>,
    pub(crate) after: std::collections::BTreeMap<crate::dom::NodeId, std::rc::Rc<ComputedStyle>>,
    pub(crate) marker: std::collections::BTreeMap<crate::dom::NodeId, std::rc::Rc<ComputedStyle>>,
    /// `::placeholder` of a text control, for the colour the empty control's hint
    /// text is painted in.
    pub(crate) placeholder:
        std::collections::BTreeMap<crate::dom::NodeId, std::rc::Rc<ComputedStyle>>,
    /// `@font-face` rules from every sheet, in order, for the fonts module.
    pub font_faces: Vec<crate::style::cascade::FontFace>,
    /// `@keyframes` by name (the last declaration of a name wins).
    pub keyframes: std::collections::BTreeMap<String, Keyframes>,
    /// What lenient mode dropped: unknown properties, invalid values, unsupported
    /// selectors and at-rules, deduplicated.
    pub unsupported: Vec<crate::Unsupported>,
    /// The viewport (from the `Media`) the styles were computed against.
    pub viewport: crate::Viewport,
    /// Elements whose `color` is the document text colour under the quirks-mode table
    /// rule, kept so an incremental restyle reproduces the full cascade.
    pub(crate) quirk_table_color: std::collections::BTreeSet<crate::dom::NodeId>,
    /// The root element's computed font size; zero until a cascade ran.
    pub(crate) root_font_size_au: Au,
    /// The matching state (hover, focus, ...) the styles were computed against.
    pub(crate) match_state: Option<crate::style::invalidation::MatchState>,
}

impl StyleSet {
    pub fn new() -> Self {
        Self::default()
    }
    /// The root element's computed font size (what `rem` resolves against), or the
    /// UA default when there is no styled root.
    pub fn root_font_size(&self) -> Au {
        if self.root_font_size_au.is_zero() {
            Au::from_px_i32(16)
        } else {
            self.root_font_size_au
        }
    }
    pub fn viewport(&self) -> crate::Viewport {
        self.viewport
    }
    /// Number of node slots (styled or not).
    pub fn len(&self) -> usize {
        self.styles.len()
    }
    pub fn is_empty(&self) -> bool {
        self.styles.iter().all(|s| s.is_none())
    }
    pub fn record_unsupported(&mut self, u: crate::Unsupported) {
        if !self.unsupported.contains(&u) {
            self.unsupported.push(u);
        }
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
    pub fn set_placeholder(&mut self, id: crate::dom::NodeId, style: std::rc::Rc<ComputedStyle>) {
        self.placeholder.insert(id, style);
    }
    pub fn placeholder(&self, id: crate::dom::NodeId) -> Option<&ComputedStyle> {
        self.placeholder.get(&id).map(|s| &**s)
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
    /// The first difference between this set and `other` over the nodes connected to
    /// `doc` (what an incremental restyle must reproduce from a full cascade), or
    /// `None` when they agree. Entries for nodes no longer in the document, and the
    /// `unsupported` log, are not compared.
    pub fn diff(&self, other: &StyleSet, doc: &crate::dom::Document) -> Option<String> {
        if self.root_font_size_au != other.root_font_size_au {
            return Some("root font size".into());
        }
        if self.font_faces != other.font_faces {
            return Some("@font-face rules".into());
        }
        if self.keyframes != other.keyframes {
            return Some("@keyframes".into());
        }
        if self.viewport != other.viewport {
            return Some("viewport".into());
        }
        for n in doc.descendants(crate::dom::Document::ROOT) {
            type Entry<'a> = (
                Option<&'a ComputedStyle>,
                Option<&'a ComputedStyle>,
                Option<&'a ComputedStyle>,
                Option<&'a ComputedStyle>,
                Option<&'a ComputedStyle>,
                bool,
            );
            fn what(s: &StyleSet, n: crate::dom::NodeId) -> Entry<'_> {
                (
                    s.get(n),
                    s.before(n),
                    s.after(n),
                    s.marker(n),
                    s.placeholder(n),
                    s.quirk_table_color.contains(&n),
                )
            }
            let (a, b) = (what(self, n), what(other, n));
            if a != b {
                let tag = doc.tag(n).unwrap_or("#text");
                let part = if a.0 != b.0 {
                    let fields = match (a.0, b.0) {
                        (Some(x), Some(y)) => {
                            let (x, y) = (format!("{x:#?}"), format!("{y:#?}"));
                            x.lines()
                                .zip(y.lines())
                                .filter(|(l, r)| l != r)
                                .take(4)
                                .map(|(l, r)| format!("{} != {}", l.trim(), r.trim()))
                                .collect::<Vec<_>>()
                                .join("; ")
                        }
                        (x, y) => format!("present {} != {}", x.is_some(), y.is_some()),
                    };
                    format!("style ({fields})")
                } else if a.1 != b.1 {
                    "::before".into()
                } else if a.2 != b.2 {
                    "::after".into()
                } else if a.3 != b.3 {
                    "::marker".into()
                } else if a.4 != b.4 {
                    "::placeholder".into()
                } else {
                    "quirks table colour".into()
                };
                return Some(format!("node {} <{tag}>: {part}", n.0));
            }
        }
        None
    }
    pub fn clear(&mut self, id: crate::dom::NodeId) {
        if let Some(s) = self.styles.get_mut(id.index()) {
            *s = None;
        }
        self.before.remove(&id);
        self.after.remove(&id);
        self.marker.remove(&id);
        self.placeholder.remove(&id);
    }
}
