//! Lumen analytics dashboard, hand-positioned with the Painter.
use cw_applications::desktop_scene::shared::Align;
use cw_applications::desktop_scene::Painter;
use cw_scene::{Color, Rect};

const BG: Color = Color(249, 250, 251, 255);
const WHITE: Color = Color(255, 255, 255, 255);
const BORDER: Color = Color(229, 231, 235, 255);
const INK: Color = Color(17, 24, 39, 255);
const MUTED: Color = Color(107, 114, 128, 255);
const FAINT: Color = Color(156, 163, 175, 255);
const INDIGO: Color = Color(79, 70, 229, 255);
const INDIGO_SOFT: Color = Color(238, 242, 255, 255);
const GREEN: Color = Color(4, 120, 87, 255);
const GREEN_SOFT: Color = Color(236, 253, 245, 255);
const RED: Color = Color(190, 18, 60, 255);
const RED_SOFT: Color = Color(255, 241, 242, 255);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Week,
    Month,
    Quarter,
}

struct RangeData {
    label: &'static str,
    phrase: &'static str,
    kpis: [(&'static str, &'static str, f32); 4],
    points: &'static [f32],
    labels: &'static [&'static str],
}

fn data(r: Range) -> RangeData {
    match r {
        Range::Week => RangeData {
            label: "Last 7 days",
            phrase: "this week",
            kpis: [
                ("Revenue", "$18,240", 4.2),
                ("Orders", "412", 2.9),
                ("New customers", "96", -3.1),
                ("Refund rate", "1.8%", -0.4),
            ],
            points: &[21.0, 24.0, 22.0, 28.0, 26.0, 31.0, 34.0],
            labels: &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
        },
        Range::Month => RangeData {
            label: "Last 30 days",
            phrase: "this month",
            kpis: [
                ("Revenue", "$84,512", 12.4),
                ("Orders", "1,906", 8.1),
                ("New customers", "438", 5.6),
                ("Refund rate", "2.1%", -0.9),
            ],
            points: &[42.0, 48.0, 45.0, 53.0, 61.0, 58.0, 66.0, 72.0],
            labels: &["Apr 1", "Apr 5", "Apr 9", "Apr 13", "Apr 17", "Apr 21", "Apr 25", "Apr 29"],
        },
        Range::Quarter => RangeData {
            label: "Last 90 days",
            phrase: "this quarter",
            kpis: [
                ("Revenue", "$241,090", 18.9),
                ("Orders", "5,771", 15.2),
                ("New customers", "1,204", -2.3),
                ("Refund rate", "2.4%", 0.6),
            ],
            points: &[
                128.0, 141.0, 136.0, 155.0, 170.0, 164.0, 188.0, 203.0, 196.0, 221.0, 236.0, 249.0,
            ],
            labels: &["W1", "W2", "W3", "W4", "W5", "W6", "W7", "W8", "W9", "W10", "W11", "W12"],
        },
    }
}

pub struct Dashboard {
    pub range: Range,
    pub menu_open: bool,
    pub tab: usize,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self {
            range: Range::Month,
            menu_open: false,
            tab: 0,
        }
    }
}

const ORDERS: [(&str, &str, &str, &str, &str, &str, Color); 5] = [
    ("#3210", "Olivia Martin", "olivia@example.com", "Apr 29, 2024", "Paid", "$1999.00", Color(139, 92, 246, 255)),
    ("#3209", "Jackson Lee", "jackson@example.com", "Apr 28, 2024", "Pending", "$39.00", Color(14, 165, 233, 255)),
    ("#3208", "Isabella Nguyen", "isabella@example.com", "Apr 28, 2024", "Paid", "$299.00", Color(16, 185, 129, 255)),
    ("#3207", "William Kim", "will@example.com", "Apr 27, 2024", "Refunded", "$99.00", Color(245, 158, 11, 255)),
    ("#3206", "Sofia Davis", "sofia@example.com", "Apr 26, 2024", "Failed", "$450.50", Color(244, 63, 94, 255)),
];

impl Dashboard {
    pub fn click(&mut self, target: &str) {
        match target {
            "range" => self.menu_open = !self.menu_open,
            "range:7d" => {
                self.range = Range::Week;
                self.menu_open = false;
            }
            "range:30d" => {
                self.range = Range::Month;
                self.menu_open = false;
            }
            "range:90d" => {
                self.range = Range::Quarter;
                self.menu_open = false;
            }
            t if t.starts_with("tab:") => self.tab = t[4..].parse().unwrap_or(0),
            _ => {}
        }
    }

    pub fn render(&self, p: &mut Painter) {
        p.scene.background = BG;
        let d = data(self.range);
        self.sidebar(p);
        // Header
        p.box_(Rect::new(256, 0, 1024, 64), WHITE, 0);
        p.hline(256, 63, 1024, BORDER);
        p.border(Rect::new(288, 13, 448, 38), Color(249, 250, 251, 255), 8, BORDER);
        p.symbol("search", 300, 24, 16, FAINT);
        p.left(324, 23, 400, "Search orders, customers…", 14, FAINT);
        p.symbol("bell", 1161, 21, 20, MUTED);
        p.circle(1181, 22, 4, Color(244, 63, 94, 255));
        p.vline(1203, 16, 32, BORDER);
        p.circle(1232, 32, 16, Color(236, 72, 153, 255));
        p.label(1216, 25, 32, "AR", 12, WHITE, true, Align::Center);

        // Title row
        p.label(288, 98, 500, "Overview", 24, INK, true, Align::Left);
        p.left(288, 134, 500, &format!("Here's what happened with your store {}.", d.phrase), 14, MUTED);
        p.border(Rect::new(978, 114, 159, 38), WHITE, 8, BORDER);
        p.region(Rect::new(978, 114, 159, 38), "range", "Date range");
        p.symbol("calendar", 990, 125, 16, FAINT);
        p.left(1014, 124, 100, d.label, 14, Color(55, 65, 81, 255));
        p.symbol("chevron-down", 1112, 125, 16, FAINT);
        p.box_(Rect::new(1149, 114, 99, 38), INDIGO, 8);
        p.symbol("download", 1165, 125, 16, WHITE);
        p.label(1189, 124, 50, "Export", 14, WHITE, false, Align::Left);

        // Tabs
        let tabs = ["Overview", "Reports", "Customers"];
        let mut x = 292;
        for (i, t) in tabs.iter().enumerate() {
            let w = p.measure(t, 14, false);
            let on = i == self.tab;
            p.left(x, 176, w + 4, t, 14, if on { INDIGO } else { MUTED });
            p.region(Rect::new(x - 4, 170, w + 8, 40), &format!("tab:{i}"), t);
            if on {
                p.box_(Rect::new(x - 4, 207, w + 8, 2), INDIGO, 0);
            }
            x += w as i32 + 32;
        }
        p.hline(288, 209, 960, BORDER);

        if self.tab == 0 {
            // KPI cards
            let icons = ["wallet", "inbox", "person", "archive"];
            let tints = [
                (INDIGO_SOFT, INDIGO),
                (Color(240, 249, 255, 255), Color(2, 132, 199, 255)),
                (GREEN_SOFT, Color(5, 150, 105, 255)),
                (Color(255, 251, 235, 255), Color(217, 119, 6, 255)),
            ];
            for (i, (label, value, delta)) in d.kpis.iter().enumerate() {
                let cx = 288 + i as i32 * 246;
                let r = Rect::new(cx, 234, 222, 154);
                p.drop_shadow(r, 12, 2, 12, 1);
                p.border(r, WHITE, 12, BORDER);
                p.box_(Rect::new(cx + 21, 255, 40, 40), tints[i].0, 8);
                p.symbol(icons[i], cx + 31, 265, 20, tints[i].1);
                let up = *delta >= 0.0;
                let badge = format!("{}{:.1}%", if up { "+" } else { "" }, delta);
                let bw = p.measure(&badge, 12, false) + 30;
                p.box_(
                    Rect::new(cx + 201 - bw as i32, 265, bw, 20),
                    if up { GREEN_SOFT } else { RED_SOFT },
                    10,
                );
                p.symbol(
                    if up { "arrow-up" } else { "arrow-right" },
                    cx + 209 - bw as i32,
                    269,
                    12,
                    if up { GREEN } else { RED },
                );
                p.left(cx + 223 - bw as i32, 267, bw, &badge, 12, if up { GREEN } else { RED });
                p.left(cx + 21, 312, 180, label, 14, MUTED);
                p.label(cx + 21, 336, 180, value, 24, INK, true, Align::Left);
            }

            // Revenue chart card
            let card = Rect::new(288, 412, 632, 354);
            p.drop_shadow(card, 12, 2, 12, 1);
            p.border(card, WHITE, 12, BORDER);
            p.label(312, 438, 200, "Revenue", 16, INK, true, Align::Left);
            p.symbol("arrow-up", 312, 468, 16, Color(16, 185, 129, 255));
            p.left(332, 466, 300, &format!("{} {}", d.kpis[0].1, d.phrase), 14, MUTED);
            p.circle(830, 444, 4, Color(99, 102, 241, 255));
            p.left(842, 436, 60, "Net sales", 12, MUTED);
            self.line_chart(p, &d, Rect::new(312, 508, 584, 236));

            // Channel card
            let card = Rect::new(944, 412, 304, 354);
            p.drop_shadow(card, 12, 2, 12, 1);
            p.border(card, WHITE, 12, BORDER);
            p.label(968, 438, 250, "Sales by channel", 16, INK, true, Align::Left);
            p.left(968, 466, 250, "Share of orders", 14, MUTED);
            let channels = [
                ("Direct", 42, Color(99, 102, 241, 255)),
                ("Search", 28, Color(14, 165, 233, 255)),
                ("Social", 18, Color(16, 185, 129, 255)),
                ("Email", 12, Color(245, 158, 11, 255)),
            ];
            p.hline(968, 668, 256, BORDER);
            for (i, (name, v, c)) in channels.iter().enumerate() {
                let h = (*v as u32) * 126 / 42;
                let bx = 983 + i as i32 * 60;
                p.box_(Rect::new(bx, 668 - h as i32, 33, h), *c, 6);
                p.label(bx - 10, 652 - h as i32, 53, &format!("{v}%"), 11, Color(55, 65, 81, 255), true, Align::Center);
                p.center(bx - 10, 676, 53, name, 11, MUTED);
            }

            // Orders table
            let t = Rect::new(288, 790, 960, 380);
            p.border(t, WHITE, 12, BORDER);
            p.label(308, 806, 300, "Recent orders", 16, INK, true, Align::Left);
            p.symbol("more", 1212, 808, 20, FAINT);
            p.box_(Rect::new(289, 850, 958, 40), BG, 0);
            let cols = [308, 430, 740, 900];
            for (i, h) in ["ORDER", "CUSTOMER", "DATE", "STATUS"].iter().enumerate() {
                p.left(cols[i], 863, 150, h, 12, MUTED);
            }
            p.right(1100, 863, 128, "AMOUNT", 12, MUTED);
            for (row, o) in ORDERS.iter().enumerate() {
                let y = 890 + row as i32 * 56;
                p.hline(289, y, 958, BORDER);
                p.label(cols[0], y + 19, 100, o.0, 14, INK, true, Align::Left);
                p.circle(cols[1] + 16, y + 28, 16, o.6);
                let initials: String = o.1.split(' ').filter_map(|w| w.chars().next()).collect();
                p.label(cols[1], y + 21, 32, &initials, 12, WHITE, true, Align::Center);
                p.label(cols[1] + 44, y + 10, 250, o.1, 14, INK, true, Align::Left);
                p.left(cols[1] + 44, y + 30, 250, o.2, 12, MUTED);
                p.left(cols[2], y + 19, 150, o.3, 14, MUTED);
                let (bg, fg) = match o.4 {
                    "Paid" => (GREEN_SOFT, GREEN),
                    "Pending" => (Color(255, 251, 235, 255), Color(180, 83, 9, 255)),
                    "Refunded" => (BG, Color(75, 85, 99, 255)),
                    _ => (RED_SOFT, RED),
                };
                let w = p.measure(o.4, 12, false) + 16;
                p.box_(Rect::new(cols[3], y + 18, w, 20), bg, 10);
                p.left(cols[3] + 8, y + 20, w, o.4, 12, fg);
                p.label(1100, y + 19, 128, o.5, 14, INK, true, Align::Right);
            }
        } else if self.tab == 1 {
            self.reports(p);
        } else {
            p.border(Rect::new(288, 234, 960, 200), WHITE, 12, BORDER);
            p.symbol("person", 748, 282, 40, Color(209, 213, 219, 255));
            p.label(288, 336, 960, "No segments yet", 14, INK, true, Align::Center);
            p.center(288, 358, 960, "Create a segment to group customers by behaviour.", 14, MUTED);
        }

        if self.menu_open {
            let m = Rect::new(929, 160, 208, 124);
            p.z += 10;
            p.drop_shadow(m, 8, 10, 30, 4);
            p.border(m, WHITE, 8, BORDER);
            for (i, (key, r)) in [("range:7d", Range::Week), ("range:30d", Range::Month), ("range:90d", Range::Quarter)]
                .iter()
                .enumerate()
            {
                let row = Rect::new(933, 164 + i as i32 * 38, 200, 38);
                let on = *r == self.range;
                if on {
                    p.box_(row, INDIGO_SOFT, 6);
                }
                p.region(row, key, data(*r).label);
                p.left(945, row.y + 10, 150, data(*r).label, 14, if on { Color(67, 56, 202, 255) } else { Color(55, 65, 81, 255) });
                if on {
                    p.symbol("check", 1105, row.y + 11, 16, Color(67, 56, 202, 255));
                }
            }
            p.z -= 10;
        }
    }

    fn sidebar(&self, p: &mut Painter) {
        p.box_(Rect::new(0, 0, 256, 800), WHITE, 0);
        p.vline(255, 0, 800, BORDER);
        p.box_(Rect::new(24, 16, 32, 32), Color(99, 102, 241, 255), 8);
        p.symbol("magic", 32, 24, 16, WHITE);
        p.label(64, 20, 150, "Lumen", 18, INK, true, Align::Left);
        let nav = [
            ("Dashboard", "grid", None),
            ("Reports", "sliders", None),
            ("Customers", "person", None),
            ("Orders", "inbox", Some("12")),
            ("Products", "archive", None),
            ("Settings", "gear", None),
        ];
        for (i, (label, icon, count)) in nav.iter().enumerate() {
            let y = 80 + i as i32 * 40;
            let on = i == 0;
            if on {
                p.box_(Rect::new(12, y, 231, 36), INDIGO_SOFT, 8);
            }
            p.symbol(icon, 24, y + 8, 20, if on { INDIGO } else { FAINT });
            p.left(56, y + 9, 150, label, 14, if on { Color(67, 56, 202, 255) } else { Color(75, 85, 99, 255) });
            if let Some(c) = count {
                p.box_(Rect::new(203, y + 8, 28, 20), Color(243, 244, 246, 255), 10);
                p.center(203, y + 10, 28, c, 12, Color(75, 85, 99, 255));
            }
        }
        p.left(24, 344, 100, "TEAMS", 12, FAINT);
        for (i, (t, c)) in [
            ("Growth", Color(99, 102, 241, 255)),
            ("Retention", Color(16, 185, 129, 255)),
            ("Partnerships", Color(245, 158, 11, 255)),
        ]
        .iter()
        .enumerate()
        {
            let y = 370 + i as i32 * 40;
            p.circle(28, y + 16, 4, *c);
            p.left(44, y + 8, 150, t, 14, Color(75, 85, 99, 255));
        }
        p.hline(0, 731, 255, BORDER);
        p.circle(34, 766, 18, Color(236, 72, 153, 255));
        p.label(16, 758, 36, "AR", 14, WHITE, true, Align::Center);
        p.label(64, 749, 140, "Ava Reyes", 14, INK, true, Align::Left);
        p.left(64, 769, 140, "ava@lumen.app", 12, MUTED);
        p.symbol("folder", 223, 758, 16, FAINT);
    }

    fn line_chart(&self, p: &mut Painter, d: &RangeData, r: Rect) {
        let max = (d.points.iter().cloned().fold(0.0, f32::max) / 10.0).ceil() * 10.0;
        let left = r.x + 40;
        let bottom = r.y + r.height as i32 - 28;
        let top = r.y + 16;
        let right = r.x + r.width as i32 - 16;
        let inner_h = (bottom - top) as f32;
        let x_at = |i: usize| left + ((right - left) as f32 * i as f32 / (d.points.len() - 1) as f32) as i32;
        let y_at = |v: f32| bottom - (v / max * inner_h) as i32;
        for k in 0..5 {
            let v = max * k as f32 / 4.0;
            let y = y_at(v);
            let mut x = left;
            while x < right {
                p.hline(x, y, 4, BORDER);
                x += 8;
            }
            p.right(r.x, y - 8, 34, &format!("${}k", v as i32), 11, MUTED);
        }
        for (i, l) in d.labels.iter().enumerate() {
            p.center(x_at(i) - 25, bottom + 8, 50, l, 11, MUTED);
        }
        let pts: Vec<(i32, i32)> = d.points.iter().enumerate().map(|(i, v)| (x_at(i), y_at(*v))).collect();
        // Area: vertical strips fading out.
        for w in pts.windows(2) {
            let (x0, y0) = w[0];
            let (x1, y1) = w[1];
            let mut x = x0;
            while x < x1 {
                let y = y0 + (y1 - y0) * (x - x0) / (x1 - x0).max(1);
                p.gradient(
                    Rect::new(x, y, 4, (bottom - y).max(0) as u32),
                    Color(99, 102, 241, 60),
                    Color(99, 102, 241, 0),
                    8,
                );
                x += 4;
            }
        }
        p.line(pts.clone(), Color(99, 102, 241, 255), 3);
        let (lx, ly) = *pts.last().unwrap();
        p.circle(lx, ly, 6, Color(99, 102, 241, 255));
        p.circle(lx, ly, 3, WHITE);
    }

    fn reports(&self, p: &mut Painter) {
        let card = Rect::new(288, 234, 624, 330);
        p.border(card, WHITE, 12, BORDER);
        p.label(312, 258, 400, "Quarterly goals", 16, INK, true, Align::Left);
        p.left(312, 284, 500, "Progress toward the targets set in January.", 14, MUTED);
        let goals: [(&str, f32, f32, Color); 4] = [
            ("Monthly revenue", 84.0, 100.0, Color(99, 102, 241, 255)),
            ("New customers", 438.0, 500.0, Color(16, 185, 129, 255)),
            ("Average order value", 44.0, 40.0, Color(14, 165, 233, 255)),
            ("Support tickets closed", 172.0, 240.0, Color(245, 158, 11, 255)),
        ];
        for (i, (name, cur, target, c)) in goals.iter().enumerate() {
            let y = 330 + i as i32 * 54;
            p.label(312, y, 300, name, 14, Color(55, 65, 81, 255), true, Align::Left);
            p.right(700, y, 188, &format!("{} / {}", *cur as i32, *target as i32), 14, MUTED);
            p.box_(Rect::new(312, y + 28, 576, 8), Color(243, 244, 246, 255), 4);
            let w = (576.0 * (cur / target).min(1.0)) as u32;
            p.box_(Rect::new(312, y + 28, w, 8), *c, 4);
        }
        let card = Rect::new(936, 234, 312, 330);
        p.border(card, WHITE, 12, BORDER);
        p.label(960, 258, 200, "Retention", 16, INK, true, Align::Left);
        p.ring(1092, 380, 58, 12, INDIGO_SOFT);
        let arc = cw_applications::desktop_scene::shared::arc_points(1092, 380, 52, 0, 259, 6);
        p.line(arc, Color(99, 102, 241, 255), 12);
        p.label(1032, 364, 120, "72%", 24, INK, true, Align::Center);
        p.center(1032, 394, 120, "30-day", 12, MUTED);
        p.center(936, 470, 312, "Up 4.1 points since last quarter.", 14, MUTED);
    }
}
