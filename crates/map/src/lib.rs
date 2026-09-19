//! Street maps for a synthetic geography: the picture behind maps.google.com and
//! openstreetmap.org and the canvas of the native Maps app. There are no tiles to
//! serve and no real city to trace, so the map is derived from the places alone: a
//! street grid laid over the world in micro-degrees, arterials named after the streets
//! in the places' addresses, water off the western edge of the places' world, parks
//! wherever a block's hash says so, and the pins and route the caller asks for.
//!
//! The same view yields the same map on every platform and in every run, because
//! everything here is integer arithmetic: coordinates are micro-degrees and pixels,
//! the block hash is FNV-1a, and no float ever enters. The map is produced first as
//! geometry (`geometry`), typed layers in pixel coordinates a native app draws with its
//! own antialiased paths, and from that as straight-alpha RGBA8 (`rasterize`) for a
//! site to serve as a page image.

/// Straight-alpha colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);
impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }
}

/// The light-theme palette every map wears, shared by the raster and the native canvas.
pub mod palette {
    use super::Rgba;
    pub const LAND: Rgba = Rgba::rgb(242, 240, 233);
    pub const BLOCK: Rgba = Rgba::rgb(236, 234, 226);
    pub const WATER: Rgba = Rgba::rgb(170, 204, 240);
    pub const PARK: Rgba = Rgba::rgb(197, 227, 186);
    pub const STREET: Rgba = Rgba::rgb(255, 255, 255);
    pub const STREET_EDGE: Rgba = Rgba::rgb(214, 211, 203);
    pub const ARTERIAL: Rgba = Rgba::rgb(255, 245, 200);
    pub const ARTERIAL_EDGE: Rgba = Rgba::rgb(222, 200, 130);
    pub const ROUTE: Rgba = Rgba::rgb(26, 115, 232);
    pub const ROUTE_HALO: Rgba = Rgba::rgb(255, 255, 255);
    pub const PIN: Rgba = Rgba::rgb(217, 48, 37);
    pub const PIN_ROUTE: Rgba = Rgba::rgb(26, 115, 232);
    pub const PIN_RING: Rgba = Rgba::rgb(255, 255, 255);
    pub const INK: Rgba = Rgba::rgb(32, 33, 36);
    pub const STREET_INK: Rgba = Rgba::rgb(95, 99, 104);
    pub const PLATE: Rgba = Rgba(255, 255, 255, 220);
}

/// Micro-degrees of latitude between one minor street and the next (about 170 m).
pub const STEP_LAT: i64 = 1_500;
/// Micro-degrees of longitude between minor streets: the same distance on the ground,
/// longitude degrees being shorter by `LON_SCALE_PER_MIL`.
pub const STEP_LON: i64 = 2_200;
/// cos(latitude) at the world's working latitude, in parts per thousand — the geo
/// service's own correction, so a map's proportions match its distances.
pub const LON_SCALE_PER_MIL: i64 = 674;

/// A place to pin, with the street its address names (the geo service extracts it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// Micro-degrees.
    pub lat: i64,
    pub lon: i64,
    pub street: String,
}

/// A route between two points, walked latitude first and then longitude, the two legs
/// the geo service's directions describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    /// (lat, lon) in micro-degrees.
    pub from: (i64, i64),
    pub to: (i64, i64),
}

/// A box in micro-degrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bbox {
    pub min_lat: i64,
    pub min_lon: i64,
    pub max_lat: i64,
    pub max_lon: i64,
}
impl Bbox {
    /// The box around `points` ((lat, lon) pairs); `None` when there are none.
    pub fn around<I: IntoIterator<Item = (i64, i64)>>(points: I) -> Option<Bbox> {
        let mut b: Option<Bbox> = None;
        for (lat, lon) in points {
            b = Some(match b {
                None => Bbox {
                    min_lat: lat,
                    min_lon: lon,
                    max_lat: lat,
                    max_lon: lon,
                },
                Some(b) => Bbox {
                    min_lat: b.min_lat.min(lat),
                    min_lon: b.min_lon.min(lon),
                    max_lat: b.max_lat.max(lat),
                    max_lon: b.max_lon.max(lon),
                },
            });
        }
        b
    }
    /// A box `span_lat` micro-degrees tall centred on a point, as wide as it is tall on
    /// the ground.
    pub fn centred(lat: i64, lon: i64, span_lat: i64) -> Bbox {
        let span_lat = span_lat.max(2);
        let span_lon = span_lat * 1000 / LON_SCALE_PER_MIL;
        Bbox {
            min_lat: lat - span_lat / 2,
            min_lon: lon - span_lon / 2,
            max_lat: lat + span_lat / 2,
            max_lon: lon + span_lon / 2,
        }
    }
    /// Grown by `percent` of its size on every side, and to at least `min_lat` tall so a
    /// single place still has streets around it.
    pub fn padded(self, percent: i64, min_lat: i64) -> Bbox {
        let mut b = self;
        let lat_pad = (b.max_lat - b.min_lat) * percent / 100;
        let lon_pad = (b.max_lon - b.min_lon) * percent / 100;
        b.min_lat -= lat_pad;
        b.max_lat += lat_pad;
        b.min_lon -= lon_pad;
        b.max_lon += lon_pad;
        let short = min_lat.max(2) - (b.max_lat - b.min_lat);
        if short > 0 {
            b.min_lat -= short / 2;
            b.max_lat += short - short / 2;
        }
        let short = min_lat.max(2) * 1000 / LON_SCALE_PER_MIL - (b.max_lon - b.min_lon);
        if short > 0 {
            b.min_lon -= short / 2;
            b.max_lon += short - short / 2;
        }
        b
    }
    pub fn span_lat(&self) -> i64 {
        (self.max_lat - self.min_lat).max(1)
    }
    pub fn span_lon(&self) -> i64 {
        (self.max_lon - self.min_lon).max(1)
    }
    pub fn contains(&self, lat: i64, lon: i64) -> bool {
        lat >= self.min_lat && lat <= self.max_lat && lon >= self.min_lon && lon <= self.max_lon
    }
}

/// A box of the world shown in a box of pixels. North is up and east is right, and the
/// box is widened (never cropped) so a metre is as long across as it is down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub bbox: Bbox,
    pub width: u32,
    pub height: u32,
}
impl View {
    pub fn new(bbox: Bbox, width: u32, height: u32) -> View {
        let (width, height) = (width.max(1), height.max(1));
        let (w, h) = (i64::from(width), i64::from(height));
        let (span_lat, span_lon) = (bbox.span_lat(), bbox.span_lon());
        let mut bbox = bbox;
        // Ground width and height in the same unit: micro-degrees of latitude.
        let ground_w = span_lon * LON_SCALE_PER_MIL / 1000;
        if ground_w * h < span_lat * w {
            let wanted = span_lat * w * 1000 / (h * LON_SCALE_PER_MIL);
            let grow = wanted - span_lon;
            bbox.min_lon -= grow / 2;
            bbox.max_lon += grow - grow / 2;
        } else {
            let wanted = ground_w * h / w;
            let grow = wanted - span_lat;
            bbox.min_lat -= grow / 2;
            bbox.max_lat += grow - grow / 2;
        }
        View {
            bbox,
            width,
            height,
        }
    }
    /// The pixel a coordinate lands on; outside the box it is outside the picture.
    pub fn project(&self, lat: i64, lon: i64) -> (i32, i32) {
        let x = (lon - self.bbox.min_lon) * i64::from(self.width) / self.bbox.span_lon();
        let y = i64::from(self.height)
            - (lat - self.bbox.min_lat) * i64::from(self.height) / self.bbox.span_lat();
        (
            x.clamp(-1 << 20, 1 << 20) as i32,
            y.clamp(-1 << 20, 1 << 20) as i32,
        )
    }
    /// Pixels between one minor street and the next.
    fn cell_px(&self) -> i64 {
        STEP_LAT * i64::from(self.height) / self.bbox.span_lat()
    }
}

/// What to draw.
#[derive(Clone, Debug)]
pub struct Scene<'a> {
    pub view: View,
    /// Every place the caller knows; those in view are pinned, and every street name
    /// becomes an arterial whether or not its place is in view.
    pub places: &'a [Place],
    pub selected: Option<&'a str>,
    pub route: Option<Route>,
    /// The world the places live in, usually the box around all of them. The water
    /// lies west of it, so zooming and panning never move the shore.
    pub world: Bbox,
}

pub type Point = (i32, i32);
pub type Polygon = Vec<Point>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinKind {
    Place,
    Selected,
    From,
    To,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pin {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub kind: PinKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Place,
    Street,
}
/// Text with its anchor: the top-left corner (`Left`) or the top-centre (`Center`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub align: Align,
    pub kind: LabelKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arterial {
    pub name: String,
    pub points: Vec<Point>,
}

/// One layer of a map, bottom to top in the order `geometry` lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Layer {
    Water(Vec<Polygon>),
    Blocks(Vec<Polygon>),
    Parks(Vec<Polygon>),
    /// Minor streets, `width` pixels wide including a one-pixel edge each side.
    Streets {
        lines: Vec<Vec<Point>>,
        width: u32,
    },
    Arterials {
        roads: Vec<Arterial>,
        width: u32,
    },
    Route(Vec<Point>),
    Pins(Vec<Pin>),
    Labels(Vec<Label>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Map {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
}

/// FNV-1a over the bytes, stable across platforms and releases.
pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
fn cell_hash(row: i64, col: i64, salt: u8) -> u64 {
    let mut b = Vec::with_capacity(17);
    b.extend_from_slice(&row.to_le_bytes());
    b.extend_from_slice(&col.to_le_bytes());
    b.push(salt);
    hash(&b)
}

/// Whether a street name runs north-south: avenues and boulevards do, streets and roads
/// do not, and anything else is decided by its hash.
pub fn north_south(street: &str) -> bool {
    let last = street
        .rsplit(|c: char| !c.is_ascii_alphanumeric())
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    match last.as_str() {
        "ave" | "avenue" | "blvd" | "boulevard" | "way" | "hwy" | "highway" | "pkwy"
        | "parkway" => true,
        "st" | "street" | "rd" | "road" | "dr" | "drive" | "pl" | "place" | "ln" | "lane"
        | "row" | "ct" | "court" => false,
        _ => hash(street.as_bytes()) & 1 == 1,
    }
}
fn snap(v: i64, step: i64) -> i64 {
    (v + step / 2).div_euclid(step) * step
}

/// The shoreline's longitude at a latitude: west of the world by a sixth of its width,
/// wandering by up to the width of a block per band of latitude.
fn shore_lon(world: &Bbox, lat: i64) -> i64 {
    const BAND: i64 = 6 * STEP_LAT;
    let base = world.min_lon - (world.span_lon() / 6).max(2 * STEP_LON);
    let band = lat.div_euclid(BAND);
    let wobble = |b: i64| (cell_hash(b, 0, 7) % (3 * STEP_LON as u64)) as i64;
    let (a, b) = (wobble(band), wobble(band + 1));
    let t = lat - band * BAND;
    base - a - (b - a) * t / BAND
}

/// The map as typed layers in the view's pixel coordinates, bottom layer first.
pub fn geometry(scene: &Scene) -> Map {
    let view = scene.view;
    let bbox = view.bbox;
    let (w, h) = (view.width as i32, view.height as i32);
    let mut layers = Vec::new();
    let cell_px = view.cell_px();

    // Water: a polygon along the shore, from above the view to below it, closed far west.
    {
        let top = snap(bbox.max_lat, 6 * STEP_LAT) + 6 * STEP_LAT;
        let bottom = snap(bbox.min_lat, 6 * STEP_LAT) - 6 * STEP_LAT;
        let mut pts = Vec::new();
        let mut lat = top;
        while lat >= bottom {
            pts.push(view.project(lat, shore_lon(&scene.world, lat)));
            lat -= 6 * STEP_LAT;
        }
        let west = view.project(bottom, bbox.min_lon - bbox.span_lon()).0;
        pts.push((west, pts.last().map_or(h, |p| p.1)));
        pts.push((west, pts[0].1));
        let clipped = clip_polygon(&pts, w, h);
        if clipped.len() >= 3 {
            layers.push(Layer::Water(vec![clipped]));
        }
    }

    // The street grid at the level this zoom can show: finer than a block when a block
    // is wide, every third or ninth street when it is narrow, none when it is a speck.
    let grid: Option<(i64, i64, u32)> = if cell_px >= 96 {
        Some((STEP_LAT / 3, STEP_LON / 3, 5))
    } else if cell_px >= 14 {
        Some((STEP_LAT, STEP_LON, 3))
    } else if cell_px * 3 >= 14 {
        Some((STEP_LAT * 3, STEP_LON * 3, 3))
    } else if cell_px * 9 >= 14 {
        Some((STEP_LAT * 9, STEP_LON * 9, 2))
    } else {
        None
    };
    let on_land = |lat: i64, lon: i64| lon > shore_lon(&scene.world, lat);

    if let Some((step_lat, step_lon, width)) = grid {
        // Blocks and parks: one polygon per cell of the grid, chosen by the cell's hash.
        let row0 = bbox.min_lat.div_euclid(step_lat) - 1;
        let row1 = bbox.max_lat.div_euclid(step_lat) + 1;
        let col0 = bbox.min_lon.div_euclid(step_lon) - 1;
        let col1 = bbox.max_lon.div_euclid(step_lon) + 1;
        let mut blocks = Vec::new();
        let mut parks = Vec::new();
        for row in row0..=row1 {
            for col in col0..=col1 {
                let (lat0, lon0) = (row * step_lat, col * step_lon);
                let (lat1, lon1) = (lat0 + step_lat, lon0 + step_lon);
                if !on_land(lat0, lon0) || !on_land(lat1, lon1) {
                    continue;
                }
                let inset_lat = step_lat / 12;
                let inset_lon = step_lon / 12;
                let poly = vec![
                    view.project(lat1 - inset_lat, lon0 + inset_lon),
                    view.project(lat1 - inset_lat, lon1 - inset_lon),
                    view.project(lat0 + inset_lat, lon1 - inset_lon),
                    view.project(lat0 + inset_lat, lon0 + inset_lon),
                ];
                let poly = clip_polygon(&poly, w, h);
                if poly.len() < 3 {
                    continue;
                }
                let roll = cell_hash(row, col, 1) % 100;
                if roll < 9 {
                    parks.push(poly);
                } else if roll < 55 {
                    blocks.push(poly);
                }
            }
        }
        layers.push(Layer::Blocks(blocks));
        layers.push(Layer::Parks(parks));

        // Minor streets: the grid, drawn edge to edge, stopping at the water.
        let mut lines = Vec::new();
        let mut lat = snap(bbox.min_lat, step_lat) - step_lat;
        while lat <= bbox.max_lat + step_lat {
            let y = view.project(lat, bbox.min_lon).1;
            if (0..h).contains(&y) {
                let from = view.project(lat, shore_lon(&scene.world, lat)).0.max(0);
                if from < w {
                    lines.push(vec![(from, y), (w, y)]);
                }
            }
            lat += step_lat;
        }
        let mut lon = snap(bbox.min_lon, step_lon) - step_lon;
        while lon <= bbox.max_lon + step_lon {
            let x = view.project(bbox.min_lat, lon).0;
            if (0..w).contains(&x) {
                let mut segments: Vec<Vec<Point>> = Vec::new();
                let mut open: Option<i32> = None;
                let mut lat = bbox.max_lat + STEP_LAT;
                while lat >= bbox.min_lat - STEP_LAT {
                    let y = view.project(lat, lon).1.clamp(0, h);
                    match (on_land(lat, lon), open) {
                        (true, None) => open = Some(y),
                        (false, Some(y0)) => {
                            segments.push(vec![(x, y0), (x, y)]);
                            open = None;
                        }
                        _ => {}
                    }
                    lat -= STEP_LAT;
                }
                if let Some(y0) = open {
                    segments.push(vec![(x, y0), (x, h)]);
                }
                lines.extend(segments.into_iter().filter(|s| s[0].1 != s[1].1));
            }
            lon += step_lon;
        }
        layers.push(Layer::Streets { lines, width });
    }

    // Arterials: one per street name, through the first place that names it, snapped to
    // the grid so the route's legs (which follow the places' own streets) sit on them.
    let mut roads: Vec<Arterial> = Vec::new();
    let mut street_labels = Vec::new();
    let arterial_width: u32 = if grid.is_some() { 6 } else { 3 };
    for place in scene.places {
        let name = place.street.trim();
        if name.is_empty() || roads.iter().any(|r| r.name == name) {
            continue;
        }
        let points = if north_south(name) {
            let lon = snap(place.lon, STEP_LON);
            let x = view.project(bbox.min_lat, lon).0;
            if !(0..w).contains(&x) {
                continue;
            }
            let top = view
                .project(shore_top(&scene.world, lon, bbox.max_lat), lon)
                .1;
            vec![(x, top.max(0)), (x, h)]
        } else {
            let lat = snap(place.lat, STEP_LAT);
            let y = view.project(lat, bbox.min_lon).1;
            if !(0..h).contains(&y) {
                continue;
            }
            let from = view.project(lat, shore_lon(&scene.world, lat)).0.max(0);
            if from >= w {
                continue;
            }
            vec![(from, y), (w, y)]
        };
        if grid.is_some() {
            // Names sit at the road's near end, each parallel road's a step further along
            // so neighbours do not print over each other.
            let (x, y) = points[0];
            let vertical = points[0].0 == points[1].0;
            let step = street_labels
                .iter()
                .filter(|l: &&Label| l.x == x + arterial_width as i32 / 2 + 3 || !vertical)
                .count() as i32;
            let (lx, ly, align) = if vertical {
                let stagger = street_labels.iter().filter(|l| l.y <= 6 + 12 * 8).count() as i32;
                (
                    x + arterial_width as i32 / 2 + 3,
                    (y + 6 + 12 * (stagger % 4)).max(6),
                    Align::Left,
                )
            } else {
                (
                    (x + 10 + 90 * (step % 3)).max(6),
                    y - arterial_width as i32 / 2 - 10,
                    Align::Left,
                )
            };
            street_labels.push(Label {
                text: name.to_string(),
                x: lx,
                y: ly,
                align,
                kind: LabelKind::Street,
            });
        }
        roads.push(Arterial {
            name: name.to_string(),
            points,
        });
    }
    layers.push(Layer::Arterials {
        roads,
        width: arterial_width,
    });

    // The route: latitude first, then longitude, exactly as the directions read.
    if let Some(route) = scene.route {
        let (ax, ay) = view.project(route.from.0, route.from.1);
        let (bx, by) = view.project(route.to.0, route.to.1);
        let pts = clip_polyline(&[(ax, ay), (ax, by), (bx, by)], w, h);
        if pts.len() >= 2 {
            layers.push(Layer::Route(pts));
        }
    }

    // Pins for the places in view, then their names when there are few enough to read.
    let margin = Bbox {
        min_lat: bbox.min_lat - bbox.span_lat() / 20,
        min_lon: bbox.min_lon - bbox.span_lon() / 20,
        max_lat: bbox.max_lat + bbox.span_lat() / 20,
        max_lon: bbox.max_lon + bbox.span_lon() / 20,
    };
    let mut pins = Vec::new();
    for place in scene.places {
        if !margin.contains(place.lat, place.lon) {
            continue;
        }
        let (x, y) = view.project(place.lat, place.lon);
        let at = (place.lat, place.lon);
        let kind = if scene.selected == Some(place.id.as_str()) {
            PinKind::Selected
        } else if scene.route.map(|r| r.from) == Some(at) {
            PinKind::From
        } else if scene.route.map(|r| r.to) == Some(at) {
            PinKind::To
        } else {
            PinKind::Place
        };
        pins.push(Pin {
            id: place.id.clone(),
            name: place.name.clone(),
            x,
            y,
            kind,
        });
    }
    // Route ends that are not places still get their pins.
    if let Some(route) = scene.route {
        for (at, kind) in [(route.from, PinKind::From), (route.to, PinKind::To)] {
            if pins.iter().any(|p| view.project(at.0, at.1) == (p.x, p.y)) {
                continue;
            }
            let (x, y) = view.project(at.0, at.1);
            pins.push(Pin {
                id: String::new(),
                name: String::new(),
                x,
                y,
                kind,
            });
        }
    }
    // Names: street names first, then places — the selected place and the route's ends
    // before the rest, and a name that would print over another is left off (it is in
    // the list beside the map anyway) unless it is one of those.
    let few = pins.len() <= 12;
    let mut labels = street_labels;
    let mut taken: Vec<(i32, i32, i32, i32)> = labels.iter().map(label_box).collect();
    let mut order: Vec<&Pin> = pins.iter().filter(|p| !p.name.is_empty()).collect();
    order.sort_by_key(|p| (p.kind == PinKind::Place, p.kind == PinKind::From));
    for pin in order {
        let important = pin.kind != PinKind::Place;
        if !important && !few {
            continue;
        }
        let below = pin.y + pin_radius(pin.kind) as i32 + 4;
        let above = pin.y - pin_radius(pin.kind) as i32 - 4 - 7;
        let mut label = Label {
            text: pin.name.clone(),
            x: pin.x,
            y: below,
            align: Align::Center,
            kind: LabelKind::Place,
        };
        let overlaps = |b: (i32, i32, i32, i32), taken: &[(i32, i32, i32, i32)]| {
            taken
                .iter()
                .any(|t| b.0 < t.2 && t.0 < b.2 && b.1 < t.3 && t.1 < b.3)
        };
        let mut placed = !overlaps(label_box(&label), &taken);
        if !placed {
            label.y = above;
            placed = !overlaps(label_box(&label), &taken);
        }
        if !placed && important {
            label.y = below;
            placed = true;
        }
        if placed {
            taken.push(label_box(&label));
            labels.push(label);
        }
    }
    layers.push(Layer::Pins(pins));
    layers.push(Layer::Labels(labels));
    Map {
        width: view.width,
        height: view.height,
        layers,
    }
}

/// The latitude, at or above `max_lat`, where a north-south line at `lon` leaves the water.
fn shore_top(world: &Bbox, lon: i64, max_lat: i64) -> i64 {
    let mut lat = snap(max_lat, STEP_LAT) + 2 * STEP_LAT;
    let floor = lat - 40 * STEP_LAT;
    while lat > floor && lon <= shore_lon(world, lat) {
        lat -= STEP_LAT;
    }
    lat
}

/// The pixels a label's plate covers in the bundled face, (left, top, right, bottom).
fn label_box(label: &Label) -> (i32, i32, i32, i32) {
    let (w, h) = label_size(label);
    let x = match label.align {
        Align::Left => label.x,
        Align::Center => label.x - w / 2,
    };
    (x - 3, label.y - 2, x + w + 3, label.y + h + 2)
}
/// Width and height of a label's text in the bundled face.
fn label_size(label: &Label) -> (i32, i32) {
    match label.kind {
        LabelKind::Place => (text_width(&label.text, 1, true), 7),
        LabelKind::Street => (text_width(&label.text, 1, false), 7),
    }
}
/// Radius of a pin's head in pixels.
pub fn pin_radius(kind: PinKind) -> u32 {
    match kind {
        PinKind::Selected => 9,
        PinKind::From | PinKind::To => 7,
        PinKind::Place => 6,
    }
}

// ---- clipping ---------------------------------------------------------------------

/// Sutherland–Hodgman against the pixel box, in integers (an edge's crossing is rounded
/// to the nearest pixel).
fn clip_polygon(pts: &[Point], w: i32, h: i32) -> Polygon {
    let mut out: Vec<(i64, i64)> = pts
        .iter()
        .map(|&(x, y)| (i64::from(x), i64::from(y)))
        .collect();
    let bounds = [(0, i64::from(w)), (0, i64::from(h))];
    for (axis, (low, high)) in bounds.into_iter().enumerate() {
        for side in 0..2 {
            if out.is_empty() {
                break;
            }
            let limit = if side == 0 { low } else { high };
            let inside = |p: (i64, i64)| {
                let v = if axis == 0 { p.0 } else { p.1 };
                if side == 0 {
                    v >= limit
                } else {
                    v <= limit
                }
            };
            let cross = |a: (i64, i64), b: (i64, i64)| -> (i64, i64) {
                let (av, bv) = if axis == 0 { (a.0, b.0) } else { (a.1, b.1) };
                let (ao, bo) = if axis == 0 { (a.1, b.1) } else { (a.0, b.0) };
                let d = bv - av;
                let o = if d == 0 {
                    ao
                } else {
                    ao + (bo - ao) * (limit - av) / d
                };
                if axis == 0 {
                    (limit, o)
                } else {
                    (o, limit)
                }
            };
            let input = std::mem::take(&mut out);
            let n = input.len();
            for i in 0..n {
                let cur = input[i];
                let prev = input[(i + n - 1) % n];
                match (inside(cur), inside(prev)) {
                    (true, true) => out.push(cur),
                    (true, false) => {
                        out.push(cross(prev, cur));
                        out.push(cur);
                    }
                    (false, true) => out.push(cross(prev, cur)),
                    (false, false) => {}
                }
            }
        }
    }
    let mut poly: Polygon = out.into_iter().map(|(x, y)| (x as i32, y as i32)).collect();
    poly.dedup();
    if poly.len() > 1 && poly.first() == poly.last() {
        poly.pop();
    }
    poly
}

/// A polyline clamped to the pixel box, for axis-aligned legs (a leg that lies entirely
/// outside collapses onto the edge, which draws nothing visible).
fn clip_polyline(pts: &[Point], w: i32, h: i32) -> Vec<Point> {
    pts.iter()
        .map(|&(x, y)| (x.clamp(0, w), y.clamp(0, h)))
        .collect()
}

// ---- rasterizer -------------------------------------------------------------------

struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u8>,
}
impl Canvas {
    fn new(w: u32, h: u32, bg: Rgba) -> Canvas {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for p in px.chunks_mut(4) {
            p.copy_from_slice(&[bg.0, bg.1, bg.2, 255]);
        }
        Canvas {
            w: w as i32,
            h: h as i32,
            px,
        }
    }
    fn blend(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        let a = u32::from(c.3);
        if a == 255 {
            self.px[i..i + 3].copy_from_slice(&[c.0, c.1, c.2]);
            return;
        }
        for (k, v) in [c.0, c.1, c.2].into_iter().enumerate() {
            let d = u32::from(self.px[i + k]);
            self.px[i + k] = ((d * (255 - a) + u32::from(v) * a) / 255) as u8;
        }
    }
    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        for y in y0.max(0)..y1.min(self.h) {
            for x in x0.max(0)..x1.min(self.w) {
                self.blend(x, y, c);
            }
        }
    }
    /// Even-odd fill, sampling each pixel at its centre.
    fn polygon(&mut self, pts: &[Point], c: Rgba) {
        if pts.len() < 3 {
            return;
        }
        let (mut y0, mut y1) = (i32::MAX, i32::MIN);
        for p in pts {
            y0 = y0.min(p.1);
            y1 = y1.max(p.1);
        }
        let mut xs: Vec<i64> = Vec::new();
        for y in y0.max(0)..=(y1.min(self.h - 1)) {
            let sy = i64::from(y) * 2 + 1;
            xs.clear();
            for i in 0..pts.len() {
                let (ax, ay) = (i64::from(pts[i].0) * 2, i64::from(pts[i].1) * 2);
                let j = (i + 1) % pts.len();
                let (bx, by) = (i64::from(pts[j].0) * 2, i64::from(pts[j].1) * 2);
                if (ay > sy) != (by > sy) {
                    xs.push(ax + (sy - ay) * (bx - ax) / (by - ay));
                }
            }
            xs.sort_unstable();
            for pair in xs.chunks(2) {
                if let [a, b] = pair {
                    // Pixels whose centre 2x+1 lies in [a, b).
                    let start = (a.div_euclid(2)).max(0) as i32;
                    let end = ((b + 1).div_euclid(2)).min(i64::from(self.w)) as i32;
                    for x in start..end {
                        let cx = i64::from(x) * 2 + 1;
                        if cx >= *a && cx < *b {
                            self.blend(x, y, c);
                        }
                    }
                }
            }
        }
    }
    /// A stroke of `width` pixels with round joins and caps.
    fn polyline(&mut self, pts: &[Point], width: u32, c: Rgba) {
        let r2 = i64::from(width) * i64::from(width); // (2 * width/2)^2 in doubled coords
        let reach = (width as i32 + 1) / 2 + 1;
        for seg in pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let (x0, x1) = (
                (a.0.min(b.0) - reach).max(0),
                (a.0.max(b.0) + reach).min(self.w - 1),
            );
            let (y0, y1) = (
                (a.1.min(b.1) - reach).max(0),
                (a.1.max(b.1) + reach).min(self.h - 1),
            );
            let (ax, ay) = (i64::from(a.0) * 2, i64::from(a.1) * 2);
            let (bx, by) = (i64::from(b.0) * 2, i64::from(b.1) * 2);
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = dx * dx + dy * dy;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (i64::from(x) * 2 + 1, i64::from(y) * 2 + 1);
                    let (apx, apy) = (px - ax, py - ay);
                    let d2 = if len2 == 0 {
                        apx * apx + apy * apy
                    } else {
                        let t = (apx * dx + apy * dy).clamp(0, len2);
                        let (cx, cy) = (ax + dx * t / len2, ay + dy * t / len2);
                        (px - cx) * (px - cx) + (py - cy) * (py - cy)
                    };
                    if d2 <= r2 {
                        self.blend(x, y, c);
                    }
                }
            }
        }
    }
    fn circle(&mut self, cx: i32, cy: i32, r: u32, c: Rgba) {
        let r = r as i32;
        let r2 = i64::from(r) * i64::from(r) * 4;
        for y in (cy - r).max(0)..=(cy + r).min(self.h - 1) {
            for x in (cx - r).max(0)..=(cx + r).min(self.w - 1) {
                let (dx, dy) = (i64::from(x - cx) * 2, i64::from(y - cy) * 2);
                if dx * dx + dy * dy <= r2 {
                    self.blend(x, y, c);
                }
            }
        }
    }
    /// Text in the bundled 5×7 face at `scale`, top-left at (x, y).
    fn text(&mut self, x: i32, y: i32, text: &str, scale: i32, bold: bool, c: Rgba) {
        let mut cx = x;
        let advance = if bold { 7 } else { 6 } * scale;
        for ch in text.chars() {
            if let Some(rows) = glyph(ch) {
                for (row, bits) in rows.iter().enumerate() {
                    for col in 0..5 {
                        if bits & (0x10 >> col) != 0 {
                            // Bold is the glyph printed twice, a pixel apart.
                            let extra = i32::from(bold) * scale;
                            self.rect(
                                cx + col * scale,
                                y + row as i32 * scale,
                                cx + (col + 1) * scale + extra,
                                y + (row as i32 + 1) * scale,
                                c,
                            );
                        }
                    }
                }
            }
            cx += advance;
        }
    }
}

/// Width of `text` in pixels at `scale` in the bundled face.
pub fn text_width(text: &str, scale: i32, bold: bool) -> i32 {
    let advance = if bold { 7 } else { 6 };
    (text.chars().count() as i32 * advance - 1).max(0) * scale
}

/// The map as `width`×`height` straight-alpha RGBA8.
pub fn rasterize(map: &Map) -> Vec<u8> {
    use palette::*;
    let mut cv = Canvas::new(map.width.max(1), map.height.max(1), LAND);
    for layer in &map.layers {
        match layer {
            Layer::Water(polys) => polys.iter().for_each(|p| cv.polygon(p, WATER)),
            Layer::Blocks(polys) => polys.iter().for_each(|p| cv.polygon(p, BLOCK)),
            Layer::Parks(polys) => polys.iter().for_each(|p| cv.polygon(p, PARK)),
            Layer::Streets { lines, width } => {
                lines
                    .iter()
                    .for_each(|l| cv.polyline(l, *width, STREET_EDGE));
                lines
                    .iter()
                    .for_each(|l| cv.polyline(l, width.saturating_sub(2).max(1), STREET));
            }
            Layer::Arterials { roads, width } => {
                roads
                    .iter()
                    .for_each(|r| cv.polyline(&r.points, *width, ARTERIAL_EDGE));
                roads
                    .iter()
                    .for_each(|r| cv.polyline(&r.points, width.saturating_sub(2).max(1), ARTERIAL));
            }
            Layer::Route(pts) => {
                cv.polyline(pts, 9, ROUTE_HALO);
                cv.polyline(pts, 5, ROUTE);
            }
            Layer::Pins(pins) => {
                for pin in pins {
                    let r = pin_radius(pin.kind);
                    let colour = match pin.kind {
                        PinKind::From | PinKind::To => PIN_ROUTE,
                        _ => PIN,
                    };
                    cv.circle(pin.x, pin.y, r + 2, PIN_RING);
                    cv.circle(pin.x, pin.y, r, colour);
                    cv.circle(pin.x, pin.y, r / 3, PIN_RING);
                }
            }
            Layer::Labels(labels) => {
                for label in labels {
                    let (bold, ink) = match label.kind {
                        LabelKind::Place => (true, INK),
                        LabelKind::Street => (false, STREET_INK),
                    };
                    let (l, t, r, b) = label_box(label);
                    cv.rect(l, t, r, b, PLATE);
                    cv.text(l + 3, label.y, &label.text, 1, bold, ink);
                }
            }
        }
    }
    cv.px
}

/// Geometry and pixels in one call.
pub fn render(scene: &Scene) -> Vec<u8> {
    rasterize(&geometry(scene))
}

// ---- the 5×7 face -------------------------------------------------------------------

/// Seven rows of five bits, most significant bit leftmost, for the printable ASCII the
/// map labels use; anything else is a blank.
fn glyph(ch: char) -> Option<[u8; 7]> {
    let rows = FACE.iter().find(|(c, _)| *c == ch).map(|(_, s)| *s)?;
    let mut out = [0u8; 7];
    for (row, chunk) in rows.as_bytes().chunks(5).take(7).enumerate() {
        out[row] = chunk
            .iter()
            .fold(0u8, |acc, b| (acc << 1) | u8::from(*b == b'#'));
    }
    Some(out)
}
const FACE: &[(char, &str)] = &[
    ('A', " ### #   ##   #######   ##   ##   #"),
    ('B', "#### #   ##   ##### #   ##   ##### "),
    ('C', " #####    #    #    #    #     ####"),
    ('D', "#### #   ##   ##   ##   ##   ##### "),
    ('E', "######    #    #### #    #    #####"),
    ('F', "######    #    #### #    #    #    "),
    ('G', " #####    #    # ####   ##   # ####"),
    ('H', "#   ##   ##   #######   ##   ##   #"),
    ('I', "#####  #    #    #    #    #  #####"),
    ('J', "    #    #    #    ##   ##   # ### "),
    ('K', "#   ##  # # #  ##   # #  #  # #   #"),
    ('L', "#    #    #    #    #    #    #####"),
    ('M', "#   ### ### # ## # ##   ##   ##   #"),
    ('N', "#   ###  ## # ##  ###   ##   ##   #"),
    ('O', " ### #   ##   ##   ##   ##   # ### "),
    ('P', "#### #   ##   ##### #    #    #    "),
    ('Q', " ### #   ##   ##   ## # ##  #  ## #"),
    ('R', "#### #   ##   ##### # #  #  # #   #"),
    ('S', " #####    #     ###     #    ##### "),
    ('T', "#####  #    #    #    #    #    #  "),
    ('U', "#   ##   ##   ##   ##   ##   # ### "),
    ('V', "#   ##   ##   ##   ##   # # #   #  "),
    ('W', "#   ##   ##   ## # ## # ### ###   #"),
    ('X', "#   ##   # # #   #   # # #   ##   #"),
    ('Y', "#   ##   # # #   #    #    #    #  "),
    ('Z', "#####    #   #   #   #   #    #####"),
    ('a', "           ###     # #####   # ####"),
    ('b', "#    #    #### #   ##   ##   ##### "),
    ('c', "           #####    #    #     ####"),
    ('d', "    #    # #####   ##   ##   # ####"),
    ('e', "           ### #   #######     ####"),
    ('f', "  ##  #   ####  #    #    #    #   "),
    ('g', "           #####   # ####    # ### "),
    ('h', "#    #    #### #   ##   ##   ##   #"),
    ('i', "  #        ##    #    #    #   ### "),
    ('j', "   #        ##    #    # #  #  ##  "),
    ('k', "#    #    #  # # #  ##   # #  #  # "),
    ('l', " ##    #    #    #    #    #   ### "),
    ('m', "          ## # # # ## # ## # ##   #"),
    ('n', "          #### #   ##   ##   ##   #"),
    ('o', "           ### #   ##   ##   # ### "),
    ('p', "          #### #   ##### #    #    "),
    ('q', "           #####   # ####    #    #"),
    ('r', "          # ## ##   #    #    #    "),
    ('s', "           #####     ###     ##### "),
    ('t', " #    #   ####  #    #    #     ## "),
    ('u', "          #   ##   ##   ##   # ####"),
    ('v', "          #   ##   ##   # # #   #  "),
    ('w', "          #   ##   ## # ## # # # # "),
    ('x', "          #   # # #   #   # # #   #"),
    ('y', "          #   ##   # ####    # ### "),
    ('z', "          #####   #   #   #   #####"),
    ('0', " ### #   ##  ### # ###  ##   # ### "),
    ('1', "  #   ##    #    #    #    #   ### "),
    ('2', " ### #   #    #   #   #   #   #####"),
    ('3', "#####   #   #     #     ##   # ### "),
    ('4', "   #   ##  # # #  # #####   #    # "),
    ('5', "######    ####     #    ##   # ### "),
    ('6', "  ##  #   #    #### #   ##   # ### "),
    ('7', "#####    #   #   #   #    #    #   "),
    ('8', " ### #   ##   # ### #   ##   # ### "),
    ('9', " ### #   ##   # ####    #   #  ##  "),
    ('.', "                          ##   ##  "),
    (',', "                     ##   ##    #  "),
    ('-', "                ###                "),
    ('\'', " ##   ##    #                      "),
    ('&', " ##  #  # #  #  ##  # # ##  #  ## #"),
    ('/', "    #    #   #   #   #   #    #    "),
    (':', "      ##   ##        ##   ##       "),
    ('(', "  #   #   #    #    #     #     #  "),
    (')', "  #     #     #    #    #   #   #  "),
    (' ', "                                   "),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn places() -> Vec<Place> {
        vec![
            Place {
                id: "northstar-hq".into(),
                name: "Northstar HQ".into(),
                kind: "office".into(),
                lat: 47_572_600,
                lon: -122_348_000,
                street: "Bayfront Ave".into(),
            },
            Place {
                id: "devcon-center".into(),
                name: "Cascade Convention Center".into(),
                kind: "venue".into(),
                lat: 47_611_400,
                lon: -122_333_000,
                street: "Pike St".into(),
            },
            Place {
                id: "marrow-point-park".into(),
                name: "Marrow Point Park".into(),
                kind: "park".into(),
                lat: 47_590_000,
                lon: -122_320_000,
                street: "Marrow Point Rd".into(),
            },
        ]
    }
    fn scene(places: &[Place], w: u32, h: u32) -> Scene<'_> {
        let world = Bbox::around(places.iter().map(|p| (p.lat, p.lon))).unwrap();
        Scene {
            view: View::new(world.padded(10, 4_000), w, h),
            places,
            selected: Some("devcon-center"),
            route: Some(Route {
                from: (places[0].lat, places[0].lon),
                to: (places[1].lat, places[1].lon),
            }),
            world,
        }
    }
    fn layer(map: &Map, pick: impl Fn(&Layer) -> bool) -> &Layer {
        map.layers.iter().find(|l| pick(l)).expect("layer")
    }

    #[test]
    fn the_same_scene_always_draws_the_same_bytes() {
        let places = places();
        let a = render(&scene(&places, 320, 200));
        let b = render(&scene(&places, 320, 200));
        assert_eq!(a, b);
        assert_eq!(a.len(), 320 * 200 * 4);
        assert!(a.chunks(4).all(|p| p[3] == 255));
        assert_eq!(
            geometry(&scene(&places, 320, 200)),
            geometry(&scene(&places, 320, 200))
        );
    }

    #[test]
    fn a_different_box_shows_different_streets() {
        let places = places();
        let s = scene(&places, 240, 160);
        let mut moved = s.clone();
        moved.view = View::new(
            Bbox::centred(places[2].lat + 20_000, places[2].lon + 30_000, 8_000),
            240,
            160,
        );
        let streets = |m: &Map| match layer(m, |l| matches!(l, Layer::Streets { .. })) {
            Layer::Streets { lines, .. } => lines.clone(),
            _ => unreachable!(),
        };
        let (a, b) = (geometry(&s), geometry(&moved));
        assert_ne!(streets(&a), streets(&b));
        assert_ne!(rasterize(&a), rasterize(&b));
        assert!(!streets(&a).is_empty());
    }

    #[test]
    fn the_route_runs_from_one_end_to_the_other_in_two_legs() {
        let places = places();
        let s = scene(&places, 320, 200);
        let map = geometry(&s);
        let from = s.view.project(places[0].lat, places[0].lon);
        let to = s.view.project(places[1].lat, places[1].lon);
        match layer(&map, |l| matches!(l, Layer::Route(_))) {
            Layer::Route(pts) => {
                assert_eq!(pts.len(), 3);
                assert_eq!(pts[0], from);
                assert_eq!(pts[2], to);
                // Latitude first: the elbow shares its x with the start.
                assert_eq!(pts[1], (from.0, to.1));
            }
            _ => unreachable!(),
        }
        // The route's colour is on the canvas at the elbow.
        let px = rasterize(&map);
        let i = ((to.1 as usize) * 320 + from.0 as usize) * 4;
        assert_eq!(&px[i..i + 3], &[26, 115, 232]);
        match layer(&map, |l| matches!(l, Layer::Pins(_))) {
            Layer::Pins(pins) => {
                assert_eq!(pins.len(), 3);
                assert!(pins.iter().any(|p| p.kind == PinKind::From));
                assert!(pins.iter().any(|p| p.kind == PinKind::Selected));
                assert!(pins.iter().any(|p| p.kind == PinKind::Place));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn arterials_are_named_by_the_places_streets_and_run_the_way_their_names_say() {
        let places = places();
        let map = geometry(&scene(&places, 320, 200));
        match layer(&map, |l| matches!(l, Layer::Arterials { .. })) {
            Layer::Arterials { roads, .. } => {
                let names: Vec<&str> = roads.iter().map(|r| r.name.as_str()).collect();
                assert_eq!(names, ["Bayfront Ave", "Pike St", "Marrow Point Rd"]);
                let ave = &roads[0].points;
                assert_eq!(ave[0].0, ave[1].0, "an avenue runs north-south");
                let st = &roads[1].points;
                assert_eq!(st[0].1, st[1].1, "a street runs east-west");
            }
            _ => unreachable!(),
        }
        assert!(north_south("Broadway Ave"));
        assert!(!north_south("Pike St"));
        assert_eq!(north_south("the main road"), north_south("the main road"));
    }

    #[test]
    fn water_lies_west_of_the_world_and_the_view_keeps_its_proportions() {
        let places = places();
        let world = Bbox::around(places.iter().map(|p| (p.lat, p.lon))).unwrap();
        // A view pushed west of every place is mostly water.
        let view = View::new(
            Bbox::centred(world.min_lat, world.min_lon - 60_000, 20_000),
            100,
            100,
        );
        let s = Scene {
            view,
            places: &places,
            selected: None,
            route: None,
            world,
        };
        let px = render(&s);
        let water = px.chunks(4).filter(|p| p[..3] == [170, 204, 240]).count();
        assert!(water > 100 * 100 / 2, "{water} water pixels");
        // A wide picture of a tall box widens the box rather than squashing it.
        let v = View::new(Bbox::centred(0, 0, 10_000), 400, 100);
        assert_eq!(v.bbox.span_lat(), 10_000);
        assert!(v.bbox.span_lon() > 40_000);
        let (cx, cy) = v.project(0, 0);
        assert!((199..=200).contains(&cx) && cy == 50, "{cx},{cy}");
    }

    #[test]
    fn labels_are_drawn_and_clipping_keeps_everything_on_the_canvas() {
        let places = places();
        let map = geometry(&scene(&places, 320, 200));
        for layer in &map.layers {
            let check =
                |p: &Point| assert!(p.0 >= 0 && p.0 <= 320 && p.1 >= 0 && p.1 <= 200, "{p:?}");
            match layer {
                Layer::Water(p) | Layer::Blocks(p) | Layer::Parks(p) => {
                    p.iter().flatten().for_each(check)
                }
                Layer::Streets { lines, .. } => lines.iter().flatten().for_each(check),
                Layer::Arterials { roads, .. } => {
                    roads.iter().flat_map(|r| &r.points).for_each(check)
                }
                Layer::Route(p) => p.iter().for_each(check),
                _ => {}
            }
        }
        match layer(&map, |l| matches!(l, Layer::Labels(_))) {
            Layer::Labels(labels) => {
                assert!(labels.iter().any(|l| l.text == "Cascade Convention Center"));
                assert!(labels.iter().any(|l| l.text == "Pike St"));
            }
            _ => unreachable!(),
        }
        assert_eq!(text_width("Pike St", 1, false), 41);
        assert_eq!(text_width("Pike St", 1, true), 48);
        assert!(glyph('A').is_some() && glyph('~').is_none());
        assert_eq!(glyph('I').unwrap()[0], 0b11111);
    }
}
