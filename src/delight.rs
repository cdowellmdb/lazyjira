//! The fun parts, all off with `preferences.delight = false`: confetti when a ticket reaches a
//! done status, a receipt of the session printed on quit, marks on active tickets nobody has
//! touched for weeks, an empty My Work that celebrates, and spooky season in late October.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

use crate::cache::{StatusRules, Ticket};
use crate::views::common::{age, fit};

const DAY: i64 = 24 * 60 * 60;
/// An active ticket untouched this long is dusty.
pub const DUSTY_AFTER: i64 = 21 * DAY;
/// An active ticket untouched this long is a fossil, and its row fades.
pub const FOSSIL_AFTER: i64 = 90 * DAY;
const YEAR: i64 = 365 * DAY;

#[derive(Debug, Default)]
pub struct Delight {
    pub enabled: bool,
    /// The local date, read once at startup; `None` in tests and when `date` can't run.
    pub today: Option<Today>,
    confetti: Option<Confetti>,
    pub tally: Tally,
}

/// What the session did, for the receipt.
#[derive(Debug, Default)]
pub struct Tally {
    /// Tickets moved from an active status to a done one, in the order Jira confirmed them.
    shipped: Vec<Shipped>,
    pub moved: usize,
    pub comments: usize,
    pub created: usize,
}

#[derive(Debug)]
struct Shipped {
    key: String,
    summary: String,
    /// How long it had sat untouched when it was shipped, in seconds.
    idle: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    Fresh,
    Dusty,
    Fossil,
}

impl Delight {
    /// Late October, from the 15th through Halloween.
    pub fn spooky(&self) -> bool {
        self.enabled
            && self
                .today
                .is_some_and(|today| today.month == 10 && today.day >= 15)
    }

    /// Records that Jira confirmed a move. `shipped` is the ticket as it was before the move
    /// when the move took it from an active status to a done one; `epic_finished` when that
    /// was the last open child of an epic, which gets the bigger celebration.
    pub fn moved(&mut self, shipped: Option<&Ticket>, epic_finished: bool, now: i64) {
        self.tally.moved += 1;
        let Some(ticket) = shipped else { return };
        self.tally.shipped.push(Shipped {
            key: ticket.key.clone(),
            summary: ticket.summary.clone(),
            idle: ticket.updated_secs().map(|at| (now - at).max(0)),
        });
        if self.enabled {
            self.confetti = Some(Confetti {
                started: Instant::now(),
                big: epic_finished,
            });
        }
    }

    /// Clears confetti that has finished falling. Returns whether the screen needs drawing:
    /// while confetti falls, and once more after, to sweep it away.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(confetti) = &self.confetti else {
            return false;
        };
        if now.duration_since(confetti.started) >= confetti.duration() {
            self.confetti = None;
        }
        true
    }

    pub fn animating(&self) -> bool {
        self.confetti.is_some()
    }

    /// Draws the falling confetti over everything in `area`, before the theme recolors it.
    pub fn draw(&self, buf: &mut Buffer, area: Rect, now: Instant) {
        let Some(confetti) = &self.confetti else {
            return;
        };
        let elapsed = now.duration_since(confetti.started).as_secs_f32();
        for (x, y, spark, color) in confetti.particles(area, elapsed) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(spark).set_fg(color);
            }
        }
    }

    /// How long `ticket` has sat untouched at `now` (Unix seconds). Done tickets, tickets
    /// whose read had no `updated`, and everything while delight is off are fresh.
    pub fn staleness(&self, rules: &StatusRules, ticket: &Ticket, now: i64) -> Staleness {
        match self.idle(rules, ticket, now) {
            Some(idle) if idle >= FOSSIL_AFTER => Staleness::Fossil,
            Some(idle) if idle >= DUSTY_AFTER => Staleness::Dusty,
            _ => Staleness::Fresh,
        }
    }

    fn idle(&self, rules: &StatusRules, ticket: &Ticket, now: i64) -> Option<i64> {
        if !self.enabled || rules.is_done(&ticket.status) {
            return None;
        }
        ticket.updated_secs().map(|at| now - at)
    }

    /// The mark the Updated column shows before a stale ticket's age. Each is two columns wide
    /// and shown as an emoji by default, so the column stays in line.
    pub fn stale_mark(&self, staleness: Staleness) -> Option<&'static str> {
        match (staleness, self.spooky()) {
            (Staleness::Fresh, _) => None,
            (Staleness::Dusty, false) => Some("💤"),
            (Staleness::Fossil, false) => Some("🦕"),
            (Staleness::Dusty, true) => Some("👻"),
            (Staleness::Fossil, true) => Some("💀"),
        }
    }

    /// A line for the ticket detail about how long a stale ticket has waited.
    pub fn stale_quip(
        &self,
        rules: &StatusRules,
        ticket: &Ticket,
        now: i64,
    ) -> Option<&'static str> {
        let idle = self.idle(rules, ticket, now)?;
        if idle < DUSTY_AFTER {
            None
        } else if self.spooky() {
            Some("Untouched for weeks. This one haunts the backlog 👻")
        } else if idle >= YEAR {
            Some("Untouched for over a year. It has had a birthday 🎂")
        } else if idle >= FOSSIL_AFTER {
            Some("Untouched for months. Older than most houseplants survive 🪴")
        } else {
            Some("Untouched for weeks. Older than a carton of milk 🥛")
        }
    }

    /// The art and caption My Work shows when nothing is left to do, a different one each day
    /// (`now` in Unix seconds), or `None` while delight is off.
    pub fn inbox_zero(&self, now: i64) -> Option<(&'static [&'static str], &'static str)> {
        if !self.enabled {
            return None;
        }
        if self.spooky() {
            return Some(GHOST);
        }
        Some(SCENES[(now / DAY).rem_euclid(SCENES.len() as i64) as usize])
    }

    /// The receipt printed after the terminal is restored on quit: what the session shipped,
    /// moved, commented and created, leaving out what it didn't. `None` while delight is off or
    /// when nothing happened.
    pub fn receipt(&self) -> Option<String> {
        let tally = &self.tally;
        if !self.enabled || tally.moved + tally.comments + tally.created == 0 {
            return None;
        }
        let stats: Vec<String> = [
            (tally.shipped.len(), "✓", " shipped"),
            (tally.moved, "↻", " moved"),
            (tally.comments, "💬", ""),
            (tally.created, "✚", " created"),
        ]
        .into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, icon, label)| format!("{icon} {count}{label}"))
        .collect();
        let mut body = vec![stats.join("  ")];
        if !tally.shipped.is_empty() {
            body.push(String::new());
        }
        for shipped in tally.shipped.iter().take(RECEIPT_SHIPPED) {
            body.push(format!("{}  {}", shipped.key, shipped.summary));
        }
        if tally.shipped.len() > RECEIPT_SHIPPED {
            body.push(format!("+ {} more", tally.shipped.len() - RECEIPT_SHIPPED));
        }
        let oldest = tally
            .shipped
            .iter()
            .filter_map(|shipped| Some((shipped.idle?, &shipped.key)))
            .filter(|(idle, _)| *idle >= DAY)
            .max_by_key(|(idle, _)| *idle);
        if let Some((idle, key)) = oldest {
            body.push(format!("oldest slain: {} ({} idle)", key, age(0, idle)));
        }

        let inner = RECEIPT_WIDTH - 2;
        let title = match self.today {
            Some(today) => format!("─ lazyjira · {} ", today.heading()),
            None => "─ lazyjira ".to_string(),
        };
        let sign_off = match self.today {
            _ if self.spooky() => "happy haunting 🎃",
            Some(today) => today.sign_off(),
            None => "see you soon",
        };
        let sign_off = format!(" {sign_off} ");
        let left = (inner - sign_off.width()) / 2;
        let mut lines = vec![format!("╭{}╮", fill(&title, inner))];
        lines.extend(
            body.iter()
                .map(|line| format!("│ {} │", fit(line, inner - 2))),
        );
        lines.push(format!(
            "╰{}{}{}╯",
            "─".repeat(left),
            sign_off,
            "─".repeat(inner - left - sign_off.width())
        ));
        Some(lines.join("\n"))
    }
}

/// The receipt's width in columns, borders included.
const RECEIPT_WIDTH: usize = 44;
/// How many shipped tickets the receipt names before "+ N more".
const RECEIPT_SHIPPED: usize = 5;

/// `text` followed by box-drawing dashes to `width` columns.
fn fill(text: &str, width: usize) -> String {
    format!("{text}{}", "─".repeat(width.saturating_sub(text.width())))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Today {
    /// 1 is Monday, 7 is Sunday.
    pub weekday: u8,
    pub month: u8,
    pub day: u8,
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl Today {
    /// The local date from `date`, or `None` when it can't be run.
    pub fn read() -> Option<Self> {
        let output = std::process::Command::new("date")
            .arg("+%u %m %d")
            .output()
            .ok()?;
        Self::parse(&String::from_utf8_lossy(&output.stdout))
    }

    /// `date +"%u %m %d"`'s output, like "5 10 23".
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split_whitespace().map(|part| part.parse::<u8>().ok());
        let today = Today {
            weekday: parts.next()??,
            month: parts.next()??,
            day: parts.next()??,
        };
        let valid = (1..=7).contains(&today.weekday)
            && (1..=12).contains(&today.month)
            && (1..=31).contains(&today.day);
        valid.then_some(today)
    }

    fn heading(self) -> String {
        format!(
            "{} {} {}",
            WEEKDAYS[self.weekday as usize - 1],
            MONTHS[self.month as usize - 1],
            self.day
        )
    }

    fn sign_off(self) -> &'static str {
        match self.weekday {
            5 => "see you Monday",
            6 | 7 => "enjoy the weekend",
            _ => "see you tomorrow",
        }
    }
}

#[derive(Debug)]
struct Confetti {
    started: Instant,
    /// An epic was finished: confetti rains over the whole screen, for longer.
    big: bool,
}

/// Each one column wide, so a spark covers exactly the cell it lands on.
const SPARKS: [char; 6] = ['✦', '✧', '*', '+', '•', '·'];
const SPARK_COLORS: [Color; 6] = [
    Color::Red,
    Color::Yellow,
    Color::Green,
    Color::Cyan,
    Color::Magenta,
    Color::Blue,
];

impl Confetti {
    fn duration(&self) -> Duration {
        Duration::from_millis(if self.big { 2600 } else { 1400 })
    }

    /// Each spark's cell, character and color `elapsed` seconds in, for those inside `area`.
    /// A small burst fountains up from the middle and falls; a big one rains from the top.
    /// Sparks dim to `·` in the last quarter.
    fn particles(&self, area: Rect, elapsed: f32) -> Vec<(u16, u16, char, Color)> {
        let (w, h) = (area.width as f32, area.height as f32);
        let count = if self.big { 160 } else { 48 };
        let fading = elapsed > self.duration().as_secs_f32() * 0.75;
        (0..count)
            .filter_map(|i| {
                let r = |k| noise(i, k);
                let (x, y) = if self.big {
                    let sway = (elapsed * 4.0 + r(3) * 6.3).sin() * 2.0;
                    let fall = (0.35 + 0.4 * r(2)) * h * elapsed;
                    (r(0) * w + sway, fall - r(1) * h * 0.8)
                } else {
                    let vx = (r(0) * 2.0 - 1.0) * w * 0.25;
                    let vy = -(0.5 + 0.5 * r(1)) * h * 0.9;
                    let gravity = h * 1.4;
                    (
                        w / 2.0 + vx * elapsed,
                        h * 0.6 + vy * elapsed + gravity * elapsed * elapsed / 2.0,
                    )
                };
                if !(0.0..w).contains(&x) || !(0.0..h).contains(&y) {
                    return None;
                }
                let spark = if fading {
                    '·'
                } else {
                    SPARKS[i as usize % SPARKS.len()]
                };
                Some((
                    area.x + x as u16,
                    area.y + y as u16,
                    spark,
                    SPARK_COLORS[(i as usize / SPARKS.len()) % SPARK_COLORS.len()],
                ))
            })
            .collect()
    }
}

/// A repeatable number in [0, 1) for spark `i`'s `k`th property (splitmix64).
fn noise(i: u64, k: u64) -> f32 {
    let mut z = (i << 8 | k).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

type Scene = (&'static [&'static str], &'static str);

const SCENES: [Scene; 4] = [
    (
        &[
            r"      |\      _,,,---,,_",
            r"ZZZzz /,`.-'`'    -.  ;-;;,_",
            r"     |,4-  ) )-,_. ,\ (  `'-'",
            r"    '---''(_/--'  `-'\_)",
        ],
        "Inbox zero. The cat approves.",
    ),
    (
        &[
            r"      ( (",
            r"       ) )",
            r"    ........",
            r"    |      |]",
            r"    \      /",
            r"     `----'",
        ],
        "Nothing assigned. Coffee's on you.",
    ),
    (
        &[
            r"     \   |   /",
            r"       .-'-.",
            r"  --- (     ) ---",
            r"       `-.-'",
            r"     /   |   \",
            r"~~~~~~~~~~~~~~~~~~~~",
        ],
        "All clear. Go touch grass.",
    ),
    (
        &[
            r"       /\",
            r"      |  |",
            r"      |()|",
            r"     /|  |\",
            r"    /_|__|_\",
            r"      /**\",
        ],
        "Inbox zero. Go build something.",
    ),
];

const GHOST: Scene = (
    &[
        r"     .-.",
        r"    (o o)  boo!",
        r"    | O \",
        r"     \   \",
        r"      `~~~'",
    ],
    "Nothing assigned. Spooky quiet...",
);

#[cfg(test)]
mod tests {
    use super::*;

    fn on(today: Option<Today>) -> Delight {
        Delight {
            enabled: true,
            today,
            ..Delight::default()
        }
    }

    fn date(weekday: u8, month: u8, day: u8) -> Option<Today> {
        Some(Today {
            weekday,
            month,
            day,
        })
    }

    /// A ticket in `status` last updated at Unix second 0.
    fn untouched(status: &str) -> Ticket {
        let mut ticket = Ticket::for_test("DEMO-7", status);
        ticket.updated = Some("1970-01-01T00:00:00.000+0000".to_string());
        ticket
    }

    #[test]
    fn spooky_season_runs_from_october_15th_through_halloween() {
        assert!(!on(date(3, 10, 14)).spooky());
        assert!(on(date(4, 10, 15)).spooky());
        assert!(on(date(6, 10, 31)).spooky());
        assert!(!on(date(7, 11, 1)).spooky());
        assert!(!on(None).spooky());
        let off = Delight {
            enabled: false,
            ..on(date(4, 10, 20))
        };
        assert!(!off.spooky());
    }

    #[test]
    fn todays_date_parses_from_date_output() {
        assert_eq!(Today::parse("5 10 23\n"), date(5, 10, 23));
        assert_eq!(Today::parse("8 10 23"), None);
        assert_eq!(Today::parse("5 13 23"), None);
        assert_eq!(Today::parse(""), None);
    }

    #[test]
    fn active_tickets_turn_dusty_at_three_weeks_and_fossil_at_ninety_days() {
        let delight = on(None);
        let rules = StatusRules::default();
        let ticket = untouched("In Progress");
        let at = |now| delight.staleness(&rules, &ticket, now);
        assert_eq!(at(DUSTY_AFTER - 1), Staleness::Fresh);
        assert_eq!(at(DUSTY_AFTER), Staleness::Dusty);
        assert_eq!(at(FOSSIL_AFTER - 1), Staleness::Dusty);
        assert_eq!(at(FOSSIL_AFTER), Staleness::Fossil);
    }

    #[test]
    fn done_tickets_and_delight_off_are_never_stale() {
        let rules = StatusRules::default();
        let done = untouched("Done");
        assert_eq!(
            on(None).staleness(&rules, &done, FOSSIL_AFTER),
            Staleness::Fresh
        );
        let active = untouched("In Progress");
        assert_eq!(
            Delight::default().staleness(&rules, &active, FOSSIL_AFTER),
            Staleness::Fresh
        );
        assert_eq!(on(None).stale_quip(&rules, &done, YEAR), None);
    }

    #[test]
    fn stale_marks_are_two_columns_and_spooky_in_season() {
        let plain = on(date(5, 10, 1));
        let spooky = on(date(5, 10, 23));
        assert_eq!(plain.stale_mark(Staleness::Fresh), None);
        assert_eq!(plain.stale_mark(Staleness::Dusty), Some("💤"));
        assert_eq!(plain.stale_mark(Staleness::Fossil), Some("🦕"));
        assert_eq!(spooky.stale_mark(Staleness::Dusty), Some("👻"));
        assert_eq!(spooky.stale_mark(Staleness::Fossil), Some("💀"));
        for mark in ["💤", "🦕", "👻", "💀"] {
            assert_eq!(mark.width(), 2, "{mark}");
        }
    }

    #[test]
    fn stale_quips_grow_with_the_wait() {
        let delight = on(None);
        let rules = StatusRules::default();
        let ticket = untouched("To Do");
        let quip = |now| delight.stale_quip(&rules, &ticket, now);
        assert_eq!(quip(DUSTY_AFTER - 1), None);
        assert!(quip(DUSTY_AFTER).unwrap().contains("carton of milk"));
        assert!(quip(FOSSIL_AFTER).unwrap().contains("houseplants"));
        assert!(quip(YEAR).unwrap().contains("birthday"));
    }

    #[test]
    fn inbox_zero_changes_scene_each_day_and_haunts_in_season() {
        let delight = on(date(5, 9, 1));
        assert_eq!(delight.inbox_zero(0), Some(SCENES[0]));
        assert_eq!(delight.inbox_zero(DAY - 1), Some(SCENES[0]));
        assert_eq!(delight.inbox_zero(DAY), Some(SCENES[1]));
        assert_eq!(delight.inbox_zero(4 * DAY), Some(SCENES[0]));
        assert_eq!(on(date(5, 10, 23)).inbox_zero(0), Some(GHOST));
        assert_eq!(Delight::default().inbox_zero(0), None);
    }

    #[test]
    fn shipping_a_ticket_starts_confetti_and_finishing_an_epic_makes_it_big() {
        let mut delight = on(None);
        delight.moved(None, false, 0);
        assert!(!delight.animating(), "a move that ships nothing");
        assert!(
            delight.receipt().unwrap().contains("│ ↻ 1 moved "),
            "no \"0 shipped\""
        );
        delight.moved(Some(&untouched("In Progress")), false, DAY);
        assert!(delight.confetti.as_ref().is_some_and(|c| !c.big));
        delight.moved(Some(&untouched("In Progress")), true, DAY);
        assert!(delight.confetti.as_ref().is_some_and(|c| c.big));
        assert_eq!(delight.tally.moved, 3);
        assert_eq!(delight.tally.shipped.len(), 2);
        assert_eq!(delight.tally.shipped[0].idle, Some(DAY));

        let mut off = Delight::default();
        off.moved(Some(&untouched("In Progress")), true, DAY);
        assert!(!off.animating());
        assert_eq!(off.tally.shipped.len(), 1, "the tally still counts");
    }

    #[test]
    fn confetti_ends_after_its_duration_with_one_more_draw() {
        let mut delight = on(None);
        delight.moved(Some(&untouched("In Progress")), false, 0);
        let started = delight.confetti.as_ref().unwrap().started;
        assert!(delight.tick(started + Duration::from_millis(1399)));
        assert!(delight.animating());
        assert!(delight.tick(started + Duration::from_millis(1400)));
        assert!(!delight.animating());
        assert!(!delight.tick(started + Duration::from_millis(1500)));
    }

    #[test]
    fn confetti_stays_inside_the_area_and_dims_at_the_end() {
        let area = Rect::new(3, 2, 80, 24);
        for big in [false, true] {
            let confetti = Confetti {
                started: Instant::now(),
                big,
            };
            let end = confetti.duration().as_secs_f32();
            let mut drawn = 0;
            for step in 0..=26 {
                let particles = confetti.particles(area, end * step as f32 / 26.0);
                drawn += particles.len();
                for (x, y, _, _) in &particles {
                    assert!(area.contains((*x, *y).into()), "{x},{y}");
                }
            }
            assert!(drawn > 0, "big: {big}");
            let late = confetti.particles(area, end * 0.9);
            assert!(late.iter().all(|(_, _, spark, _)| *spark == '·'));
        }
        for spark in SPARKS {
            assert_eq!(spark.to_string().width(), 1, "{spark}");
        }
    }

    #[test]
    fn noise_is_in_the_unit_interval() {
        for i in 0..500 {
            for k in 0..4 {
                assert!((0.0..1.0).contains(&noise(i, k)));
            }
        }
    }

    #[test]
    fn no_receipt_when_nothing_happened_or_delight_is_off() {
        assert_eq!(on(date(5, 10, 9)).receipt(), None);
        let mut off = Delight::default();
        off.tally.comments = 1;
        assert_eq!(off.receipt(), None);
    }

    #[test]
    fn the_receipt_lists_the_session_and_signs_off_by_weekday() {
        let mut delight = on(date(5, 10, 9));
        let mut login = untouched("In Progress");
        login.key = "DEMO-41".to_string();
        login.summary = "Fix login redirect".to_string();
        delight.moved(Some(&login), false, 13 * 7 * DAY);
        delight.moved(None, false, 0);
        delight.tally.comments = 2;
        let receipt = delight.receipt().unwrap();
        let expected = [
            "╭─ lazyjira · Fri Oct 9 ───────────────────╮",
            "│ ✓ 1 shipped  ↻ 2 moved  💬 2             │",
            "│                                          │",
            "│ DEMO-41  Fix login redirect              │",
            "│ oldest slain: DEMO-41 (13w idle)         │",
            "╰───────────── see you Monday ─────────────╯",
        ];
        assert_eq!(receipt, expected.join("\n"));
        for line in expected {
            assert_eq!(line.width(), RECEIPT_WIDTH, "{line}");
        }

        delight.today = date(2, 9, 1);
        assert!(delight.receipt().unwrap().contains(" see you tomorrow "));
        delight.today = date(6, 9, 5);
        assert!(delight.receipt().unwrap().contains(" enjoy the weekend "));
        delight.today = date(6, 10, 31);
        assert!(delight.receipt().unwrap().contains(" happy haunting 🎃 "));
    }

    #[test]
    fn the_receipt_names_five_shipped_tickets_then_counts_the_rest() {
        let mut delight = on(None);
        for n in 1..=6 {
            let mut ticket = untouched("To Do");
            ticket.key = format!("DEMO-{n}");
            ticket.updated = None;
            delight.moved(Some(&ticket), false, 0);
        }
        let receipt = delight.receipt().unwrap();
        assert!(receipt.contains("│ DEMO-5  "));
        assert!(!receipt.contains("DEMO-6"));
        assert!(receipt.contains("│ + 1 more "));
        assert!(!receipt.contains("oldest slain"), "no ticket had an age");
        assert!(receipt.starts_with("╭─ lazyjira ──"));
        assert!(receipt.contains(" see you soon "));
    }

    #[test]
    fn a_ticket_shipped_within_a_day_is_not_the_oldest_slain() {
        let mut delight = on(None);
        delight.moved(Some(&untouched("To Do")), false, DAY - 1);
        assert!(!delight.receipt().unwrap().contains("oldest slain"));
        delight.moved(Some(&untouched("To Do")), false, DAY);
        assert!(delight
            .receipt()
            .unwrap()
            .contains("oldest slain: DEMO-7 (1d idle)"));
    }
}
