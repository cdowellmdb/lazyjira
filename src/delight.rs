//! The fun parts, all off with `preferences.delight = false`: confetti when a ticket reaches a
//! done status, a receipt of the session printed on quit, marks on active tickets nobody has
//! touched for weeks, an empty My Work that celebrates, and spooky season in late October.

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::cache::{StatusKind, StatusRules, Ticket};
use crate::views::common::{fit, DONE_CELL};

const DAY: i64 = 24 * 60 * 60;
/// An active ticket untouched this long is dusty.
const DUSTY_AFTER: i64 = 21 * DAY;
/// An active ticket untouched this long is a fossil, and its row fades.
const FOSSIL_AFTER: i64 = 90 * DAY;
const YEAR: i64 = 365 * DAY;

#[derive(Debug, Default)]
pub struct Delight {
    pub enabled: bool,
    /// The local date, read once at startup; `None` in tests and when `date` can't run.
    pub today: Option<Today>,
    /// Which inbox-zero scene this session shows, chosen at startup so each launch can differ.
    pub scene: usize,
    confetti: Option<Confetti>,
    tally: Tally,
}

/// What the session did, for the receipt.
#[derive(Debug, Default)]
struct Tally {
    /// Tickets moved from an active status to a done one, in the order Jira confirmed them.
    shipped: Vec<Shipped>,
    moved: usize,
    comments: usize,
    created: usize,
}

#[derive(Debug)]
struct Shipped {
    key: String,
    summary: String,
    /// How long it had sat untouched when it was shipped, in seconds.
    idle: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Staleness {
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

    /// The title of the tab bar's frame.
    pub fn title(&self) -> &'static str {
        if self.spooky() {
            " lazyjira 🎃 "
        } else {
            " lazyjira "
        }
    }

    /// The footer's first hint: in spooky season, until `theme` is the spooky one, where to
    /// find it. `S` stays Settings; the hint only points there. Empty otherwise.
    pub fn theme_hint(&self, theme: &str) -> &'static str {
        if self.spooky() && !theme.eq_ignore_ascii_case(crate::theme::SPOOKY) {
            "🎃 S: settings › spooky theme  "
        } else {
            ""
        }
    }

    pub fn commented(&mut self) {
        self.tally.comments += 1;
    }

    pub fn created(&mut self, count: usize) {
        self.tally.created += count;
    }

    /// Records that Jira confirmed a move. `shipped` is the ticket as it was before the move
    /// when the move took it from an active status to a done one; `finished_epic` is the epic
    /// whose last open child that was, which gets the bigger celebration. A running epic
    /// celebration is never cut short by a smaller one.
    fn moved(&mut self, shipped: Option<&Ticket>, finished_epic: Option<&str>, now: i64) {
        self.tally.moved += 1;
        let Some(ticket) = shipped else { return };
        self.tally.shipped.push(Shipped {
            key: ticket.key.clone(),
            summary: ticket.summary.clone(),
            idle: ticket.updated_secs().map(|at| (now - at).max(0)),
        });
        let raining = self
            .confetti
            .as_ref()
            .is_some_and(|confetti| confetti.epic.is_some());
        if self.enabled && (finished_epic.is_some() || !raining) {
            self.confetti = Some(Confetti {
                started: Instant::now(),
                epic: finished_epic.map(str::to_string),
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

    /// `epic`'s progress bar (`views::common::epic_bar`'s spans) with its done cells in rainbow
    /// colors that shift with time while confetti celebrates finishing it; other runs, and the
    /// bar of any other epic, are left as they are.
    pub fn rainbow_bar(
        &self,
        epic: &str,
        bar: Vec<Span<'static>>,
        now: Instant,
    ) -> Vec<Span<'static>> {
        let Some(confetti) = self
            .confetti
            .as_ref()
            .filter(|confetti| confetti.epic.as_deref() == Some(epic))
        else {
            return bar;
        };
        let shift = (now.duration_since(confetti.started).as_millis() / 100) as usize;
        let mut cell = 0;
        bar.into_iter()
            .flat_map(|run| {
                let start = cell;
                cell += run.content.chars().count();
                if !run.content.starts_with(DONE_CELL) {
                    return vec![run];
                }
                (start..cell)
                    .map(|at| {
                        let color = SPARK_COLORS[(at + shift) % SPARK_COLORS.len()];
                        Span::styled(DONE_CELL, run.style.fg(color))
                    })
                    .collect()
            })
            .collect()
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
    fn staleness(&self, rules: &StatusRules, ticket: &Ticket, now: i64) -> Staleness {
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

    /// Whether `ticket` is a fossil at `now`, so its row fades.
    pub fn is_fossil(&self, rules: &StatusRules, ticket: &Ticket, now: i64) -> bool {
        self.staleness(rules, ticket, now) == Staleness::Fossil
    }

    /// The mark the Updated column shows before a stale ticket's age at `now`. Each is two
    /// columns wide and shown as an emoji by default, so the column stays in line.
    pub fn stale_mark(
        &self,
        rules: &StatusRules,
        ticket: &Ticket,
        now: i64,
    ) -> Option<&'static str> {
        match (self.staleness(rules, ticket, now), self.spooky()) {
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

    /// The art and caption My Work shows when nothing is left to do: this session's scene, or
    /// `None` while delight is off.
    pub fn inbox_zero(&self) -> Option<Scene> {
        if !self.enabled {
            return None;
        }
        if self.spooky() {
            return Some(GHOST);
        }
        Some(SCENES[self.scene % SCENES.len()])
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
            let days = idle / DAY;
            let unit = if days == 1 { "day" } else { "days" };
            body.push(format!("oldest slain: {key} ({days} {unit})"));
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
        let mut lines = vec![format!(
            "╭{title}{}╮",
            "─".repeat(inner.saturating_sub(title.width()))
        )];
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

impl App {
    /// Tallies a move Jira confirmed for the receipt, and celebrates it when it took `before`
    /// (the ticket as it was) from an active status to the done `status`; dropping a ticket
    /// isn't shipping it. Returns a message
    /// naming the epic the move finished, when it finished one and delight is on.
    pub fn celebrate_move(&mut self, before: Option<Ticket>, status: &str) -> Option<String> {
        let rules = self.status_rules();
        let shipped = before.filter(|ticket| {
            rules.kind(&ticket.status) == StatusKind::Active
                && rules.kind(status) == StatusKind::Done
        });
        let finished = shipped.as_ref().and_then(|ticket| {
            self.cache.epics.iter().find(|epic| {
                epic.children.iter().any(|child| child.key == ticket.key)
                    && epic.done_count(rules) == epic.total()
            })
        });
        let message =
            finished.map(|epic| format!("🎉 Epic {} complete: {}", epic.key, epic.summary));
        let epic = finished.map(|epic| epic.key.clone());
        let now = crate::local_cache::now_unix_secs() as i64;
        self.delight.moved(shipped.as_ref(), epic.as_deref(), now);
        message.filter(|_| self.delight.enabled)
    }
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
    /// The epic this finished: confetti rains over the whole screen, for longer.
    epic: Option<String>,
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
        Duration::from_millis(if self.epic.is_some() { 2600 } else { 1400 })
    }

    /// Each spark's cell, character and color `elapsed` seconds in, for those inside `area`.
    /// A small burst fountains up from the middle and falls; a big one rains from the top.
    /// Sparks dim to `·` in the last quarter.
    fn particles(&self, area: Rect, elapsed: f32) -> Vec<(u16, u16, char, Color)> {
        let (w, h) = (area.width as f32, area.height as f32);
        let count = if self.epic.is_some() { 160 } else { 48 };
        let fading = elapsed > self.duration().as_secs_f32() * 0.75;
        (0..count)
            .filter_map(|i| {
                let r = |k| noise(i, k);
                let (x, y) = if self.epic.is_some() {
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

/// Lines of art and a caption.
pub type Scene = (&'static [&'static str], &'static str);

const SCENES: [Scene; 5] = [
    (
        &[
            r"      |\      _,,,---,,_",
            r"ZZZzz /,`.-'`'    -.  ;-;;,_",
            r"     |,4-  ) )-,_. ,\ (  `'-'",
            r"    '---''(_/--'  `-'\_)",
        ],
        "Inbox zero. The cat has the keyboard now.",
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
            r" \|/                     \|/",
            r"  |`-._               _.-'|",
            r"  |    `-._________.-'    |",
            r"  |                       |",
            r"~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
        ],
        "Nothing to do. The hammock is free.",
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
        "Nothing assigned. Go build something.",
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
    use ratatui::style::Style;

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
        let rules = StatusRules::default();
        let ticket = untouched("In Progress");
        let mark = |delight: &Delight, now| delight.stale_mark(&rules, &ticket, now);
        let plain = on(date(5, 10, 1));
        let spooky = on(date(5, 10, 23));
        assert_eq!(mark(&plain, DUSTY_AFTER - 1), None);
        assert_eq!(mark(&plain, DUSTY_AFTER), Some("💤"));
        assert_eq!(mark(&plain, FOSSIL_AFTER), Some("🦕"));
        assert_eq!(mark(&spooky, DUSTY_AFTER), Some("👻"));
        assert_eq!(mark(&spooky, FOSSIL_AFTER), Some("💀"));
        assert!(!plain.is_fossil(&rules, &ticket, FOSSIL_AFTER - 1));
        assert!(plain.is_fossil(&rules, &ticket, FOSSIL_AFTER));
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
    fn inbox_zero_shows_the_sessions_scene_and_haunts_in_season() {
        let mut delight = on(date(5, 9, 1));
        assert_eq!(delight.inbox_zero(), Some(SCENES[0]));
        delight.scene = SCENES.len() - 1;
        assert_eq!(delight.inbox_zero(), Some(SCENES[SCENES.len() - 1]));
        delight.scene = SCENES.len();
        assert_eq!(delight.inbox_zero(), Some(SCENES[0]));
        assert_eq!(on(date(5, 10, 23)).inbox_zero(), Some(GHOST));
        assert_eq!(Delight::default().inbox_zero(), None);
    }

    #[test]
    fn shipping_a_ticket_starts_confetti_and_finishing_an_epic_makes_it_big() {
        let mut delight = on(None);
        delight.moved(None, None, 0);
        assert!(!delight.animating(), "a move that ships nothing");
        assert!(
            delight.receipt().unwrap().contains("│ ↻ 1 moved "),
            "no \"0 shipped\""
        );
        delight.moved(Some(&untouched("In Progress")), None, DAY);
        assert!(delight.confetti.as_ref().is_some_and(|c| c.epic.is_none()));
        delight.moved(Some(&untouched("In Progress")), Some("DEMO-300"), DAY);
        assert!(delight
            .confetti
            .as_ref()
            .is_some_and(|c| c.epic.as_deref() == Some("DEMO-300")));
        assert_eq!(delight.tally.moved, 3);
        assert_eq!(delight.tally.shipped.len(), 2);
        assert_eq!(delight.tally.shipped[0].idle, Some(DAY));

        delight.moved(Some(&untouched("In Progress")), None, DAY);
        assert!(
            delight.confetti.as_ref().unwrap().epic.is_some(),
            "a smaller burst doesn't cut the epic's rain short"
        );

        let mut off = Delight::default();
        off.moved(Some(&untouched("In Progress")), Some("DEMO-300"), DAY);
        assert!(!off.animating());
        assert_eq!(off.tally.shipped.len(), 1, "the tally still counts");
    }

    #[test]
    fn confetti_ends_after_its_duration_with_one_more_draw() {
        let mut delight = on(None);
        delight.moved(Some(&untouched("In Progress")), None, 0);
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
                epic: big.then(|| "DEMO-300".to_string()),
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
            let sparks = |at: f32| -> Vec<char> {
                let particles = confetti.particles(area, at);
                particles
                    .into_iter()
                    .map(|(_, _, spark, _)| spark)
                    .collect()
            };
            // The last quarter starts at exactly three quarters of the duration.
            assert!(
                sparks(end * 0.75).iter().any(|spark| *spark != '·'),
                "big: {big}"
            );
            let dim = sparks(end * 0.75 + 0.001);
            assert!(
                !dim.is_empty() && dim.iter().all(|spark| *spark == '·'),
                "big: {big}"
            );
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
        delight.moved(Some(&login), None, 13 * 7 * DAY);
        delight.moved(None, None, 0);
        delight.tally.comments = 2;
        let receipt = delight.receipt().unwrap();
        let expected = [
            "╭─ lazyjira · Fri Oct 9 ───────────────────╮",
            "│ ✓ 1 shipped  ↻ 2 moved  💬 2             │",
            "│                                          │",
            "│ DEMO-41  Fix login redirect              │",
            "│ oldest slain: DEMO-41 (91 days)          │",
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
    fn the_receipt_names_exactly_five_shipped_tickets_without_more() {
        let mut delight = on(None);
        for n in 1..=5 {
            let mut ticket = untouched("To Do");
            ticket.key = format!("DEMO-{n}");
            delight.moved(Some(&ticket), None, 0);
        }
        let receipt = delight.receipt().unwrap();
        for n in 1..=5 {
            assert!(receipt.contains(&format!("│ DEMO-{n}  ")), "{receipt}");
        }
        assert!(!receipt.contains("more"), "{receipt}");
    }

    #[test]
    fn the_receipt_names_five_shipped_tickets_then_counts_the_rest() {
        let mut delight = on(None);
        for n in 1..=6 {
            let mut ticket = untouched("To Do");
            ticket.key = format!("DEMO-{n}");
            ticket.updated = None;
            delight.moved(Some(&ticket), None, 0);
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
        delight.moved(Some(&untouched("To Do")), None, DAY - 1);
        assert!(!delight.receipt().unwrap().contains("oldest slain"));
        delight.moved(Some(&untouched("To Do")), None, DAY);
        assert!(delight
            .receipt()
            .unwrap()
            .contains("oldest slain: DEMO-7 (1 day)"));
    }

    #[test]
    fn only_the_finished_epics_bar_turns_rainbow() {
        let bar = || {
            vec![
                Span::styled("██", Style::default().fg(Color::Green)),
                Span::styled("░", Style::default().fg(Color::Gray)),
            ]
        };
        let mut delight = on(None);
        let started = Instant::now();
        assert_eq!(delight.rainbow_bar("DEMO-300", bar(), started), bar());
        delight.moved(Some(&untouched("To Do")), Some("DEMO-300"), 0);
        let started = delight.confetti.as_ref().unwrap().started;
        assert_eq!(delight.rainbow_bar("DEMO-100", bar(), started), bar());
        let colors = |at| {
            let bar = delight.rainbow_bar("DEMO-300", bar(), at);
            let text: String = bar.iter().map(|span| span.content.as_ref()).collect();
            assert_eq!(text, "██░");
            (bar[0].style.fg, bar[1].style.fg, bar[2].style.fg)
        };
        // The dropped run keeps its own color.
        assert_eq!(
            colors(started),
            (
                Some(SPARK_COLORS[0]),
                Some(SPARK_COLORS[1]),
                Some(Color::Gray)
            )
        );
        let later = started + Duration::from_millis(100);
        assert_eq!(
            colors(later),
            (
                Some(SPARK_COLORS[1]),
                Some(SPARK_COLORS[2]),
                Some(Color::Gray)
            )
        );
    }

    #[test]
    fn the_footer_points_to_the_spooky_theme_in_settings_until_it_is_chosen() {
        let spooky = on(date(5, 10, 23));
        assert_eq!(
            spooky.theme_hint("default"),
            "🎃 S: settings › spooky theme  "
        );
        assert_eq!(spooky.theme_hint("Spooky"), "");
        assert_eq!(on(date(5, 11, 1)).theme_hint("default"), "");
        assert_eq!(spooky.title(), " lazyjira 🎃 ");
        assert_eq!(on(date(5, 11, 1)).title(), " lazyjira ");
    }
}
