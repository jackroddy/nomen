//! Every piece of TUI state and every transition over it.
//!
//! Deliberately free of rendering and of I/O: this is the part worth testing,
//! and it stays testable only while nothing here knows what a terminal is.

use std::time::Duration;

use nomen::{
    Availability, Config, ExpandedSlot, Lang, ParseError, Pattern, Steering, Suggestion, Weights,
};

/// Which slice of the ranking is on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    English,
    Latin,
    Greek,
    All,
}

impl Tab {
    pub const ORDER: [Tab; 4] = [Tab::English, Tab::Latin, Tab::Greek, Tab::All];

    pub fn label(self) -> &'static str {
        match self {
            Tab::English => "english",
            Tab::Latin => "latin",
            Tab::Greek => "greek",
            Tab::All => "all",
        }
    }

    fn accepts(self, lang: Lang) -> bool {
        match self {
            Tab::English => lang == Lang::English,
            Tab::Latin => lang == Lang::Latin,
            Tab::Greek => lang == Lang::Greek,
            Tab::All => true,
        }
    }
}

/// Where the arrow keys are.
///
/// The screen is one vertical chain — the two input lines, the options box,
/// then the results — and up and down walk it end to end, crossing box
/// borders like any other row. Left and right move *within* whichever box
/// holds the cursor. Tab jumps whole boxes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Pattern,
    Steer,
    Options,
    Results,
}

/// The three boxes, for deciding which border to light up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Box_ {
    Search,
    Options,
    Results,
}

impl Focus {
    /// Which box the cursor is in.
    pub fn box_(self) -> Box_ {
        match self {
            Focus::Pattern | Focus::Steer => Box_::Search,
            Focus::Options => Box_::Options,
            Focus::Results => Box_::Results,
        }
    }

    /// The first row of the next box down, for tab.
    fn next_box(self) -> Focus {
        match self.box_() {
            Box_::Search => Focus::Options,
            Box_::Options => Focus::Results,
            Box_::Results => Focus::Pattern,
        }
    }

    fn prev_box(self) -> Focus {
        match self.box_() {
            Box_::Search => Focus::Results,
            Box_::Options => Focus::Pattern,
            Box_::Results => Focus::Options,
        }
    }
}

/// A modal layer over the boxes. While one is up it takes every key.
///
/// Esc opens the [menu](Overlay::Menu); everything else here is reached from
/// it, and every one of them is a floating box over the boxes below.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    Menu,
    Help,
    Weights,
    /// `exit?  [yes] [no]` — the only place quitting is offered, which is what
    /// keeps esc from being one keypress from the exit.
    Exit,
}

/// A line of the esc menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuItem {
    Help,
    Weights,
    Exit,
}

impl MenuItem {
    pub const ORDER: [MenuItem; 3] = [MenuItem::Help, MenuItem::Weights, MenuItem::Exit];

    pub fn label(self) -> &'static str {
        match self {
            MenuItem::Help => "help",
            MenuItem::Weights => "weights",
            MenuItem::Exit => "exit",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            MenuItem::Help => "pattern syntax and keys",
            MenuItem::Weights => "what the ranking rewards",
            MenuItem::Exit => "leave nomen",
        }
    }

    fn opens(self) -> Overlay {
        match self {
            MenuItem::Help => Overlay::Help,
            MenuItem::Weights => Overlay::Weights,
            MenuItem::Exit => Overlay::Exit,
        }
    }
}

/// A line of the help overlay.
pub enum HelpLine {
    Head(&'static str),
    Row(&'static str, &'static str),
    Blank,
}

/// Columns the help overlay is drawn in, borders included.
pub const HELP_WIDTH: u16 = 64;
/// Columns the key column of a help row is padded to.
pub const HELP_KEY_WIDTH: usize = 14;

/// What esc opens: the pattern syntax, the keys, and the way out.
//
// this used to sit in the search box's bottom border, where it cost two of the
// three things the border could say and was still too cramped for the syntax
// it was trying to teach
pub const HELP: &[HelpLine] = &[
    HelpLine::Head("pattern — blocks read in the order you write them"),
    HelpLine::Row("source", "a block — and a hard requirement"),
    HelpLine::Row("a|b   (a|b|c)", "either word, never both"),
    HelpLine::Row("?block", "the name may skip it"),
    HelpLine::Row("~block", "any of its letters, not just the first"),
    HelpLine::Row("a-b", "adjacent letters, all or nothing"),
    HelpLine::Blank,
    HelpLine::Head("keys"),
    HelpLine::Row("↑ ↓", "move up and down the whole screen"),
    HelpLine::Row("← →", "move within a box: options, language tabs"),
    HelpLine::Row("tab", "jump to the next box"),
    HelpLine::Row("enter", "flip a switch, or pick up a number for ↑↓"),
    HelpLine::Row("esc", "this menu"),
    HelpLine::Row("^q", "the way out"),
    HelpLine::Row("^n ^e ^a", "inherit · recursive · only available"),
    HelpLine::Row("^w", "weights"),
    HelpLine::Row("^r ^f", "re-check this name · refresh the snapshot"),
];

/// What the keyword line asks for once typing settles.
pub enum Typed {
    Search(Pattern, Steering),
    /// The line is empty; there is nothing to search for.
    Clear,
    /// The line does not parse. The results in hand stay on screen — a pattern
    /// is broken for as long as it takes to type the closing bracket.
    Invalid,
}

/// A search option, and how changing it must be answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Opt {
    Inherit,
    Recursive,
    OnlyAvailable,
    MinLen,
    MaxLen,
    MaxGaps,
    MaxPerLemma,
}

/// What a change costs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// The candidate set itself is different; the engine has to run again.
    Search,
    /// Only the ordering moved, so the results in hand can be reused.
    Resort,
    /// Nothing moved — the value was already at its limit.
    None,
}

impl Opt {
    // keep order and initials only used to live here. Both are now written
    // into the pattern itself -- order is the order the blocks appear in, and
    // `~` marks the one block that may give up an interior letter -- so a
    // global switch for either would be a second, blunter control.

    /// The switches, flipped outright by enter. Everything else is a number,
    /// picked up by enter and then stepped with up and down.
    pub const TOGGLES: [Opt; 3] = [Opt::Inherit, Opt::Recursive, Opt::OnlyAvailable];

    pub const ORDER: [Opt; 7] = [
        Opt::Inherit,
        Opt::Recursive,
        Opt::OnlyAvailable,
        Opt::MinLen,
        Opt::MaxLen,
        Opt::MaxGaps,
        Opt::MaxPerLemma,
    ];

    pub fn is_toggle(self) -> bool {
        Opt::TOGGLES.contains(&self)
    }

    /// The ctrl shortcut that flips a switch without navigating to it.
    //
    // no longer drawn beside the label -- the box is a grid now, and a `^n `
    // in front of every third cell broke the columns for a hint the help
    // overlay already carries
    pub fn key(self) -> Option<char> {
        match self {
            Opt::Inherit => Some('n'),
            Opt::Recursive => Some('e'),
            Opt::OnlyAvailable => Some('a'),
            _ => None,
        }
    }

    /// The switch a ctrl keypress flips, if any.
    pub fn from_key(c: char) -> Option<Opt> {
        Opt::TOGGLES.into_iter().find(|o| o.key() == Some(c))
    }

    /// One line on what the option does, shown in the status line while the
    /// cursor is on it — the options box has no room to explain itself.
    pub fn description(self) -> &'static str {
        match self {
            Opt::Inherit => "steer by the pattern's own words as well as the steer line",
            Opt::Recursive => "let a name expand to a phrase containing itself: GNU = Gnu Not Unix",
            Opt::OnlyAvailable => "hide names already published on crates.io",
            Opt::MinLen => "shortest name to consider, in letters",
            Opt::MaxLen => "longest name to consider, in letters",
            Opt::MaxGaps => "how many letters may go unaccounted for",
            Opt::MaxPerLemma => "how many forms of one dictionary word may appear at once",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Opt::Inherit => "inherit",
            Opt::Recursive => "recursive",
            Opt::OnlyAvailable => "only available",
            Opt::MinLen => "min length",
            Opt::MaxLen => "max length",
            Opt::MaxGaps => "max gaps",
            Opt::MaxPerLemma => "per lemma",
        }
    }
}

/// A tunable score weight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Knob {
    Name,
    Coverage,
    Usage,
    Relation,
    Niceness,
}

impl Knob {
    pub const ORDER: [Knob; 5] =
        [Knob::Name, Knob::Coverage, Knob::Usage, Knob::Relation, Knob::Niceness];

    pub fn label(self) -> &'static str {
        match self {
            Knob::Name => "name",
            Knob::Coverage => "coverage",
            Knob::Usage => "usage",
            Knob::Relation => "relation",
            Knob::Niceness => "niceness",
        }
    }

    /// A worked example of what the weight rewards.
    pub fn example(self) -> &'static [&'static str] {
        match self {
            Knob::Name => &["high  forge  muse  cargo", "low   the  able  besoot"],
            Knob::Coverage => &["?(source|code) graph", "1.0  Source Code", "0.0  _ _ Code"],
            Knob::Usage => &["1.0  Source Api Graph", "0.3  Source Code _ _ _"],
            Knob::Relation => &["for \"code graph\":", "high  logos  gnome", "low   scopa  lusca"],
            Knob::Niceness => &["high  ideal  perfect  heart", "low   errors  accident"],
        }
    }

    /// One line on what the weight actually rewards, shown while it is picked.
    pub fn description(self) -> &'static str {
        match self {
            Knob::Name => "how recognizable the word is as a name on its own",
            Knob::Coverage => "how many ?optional blocks the name uses; inert without one",
            Knob::Usage => "how much of the name keywords account for; gaps cost here",
            Knob::Relation => "how close the name's meaning is to your keywords",
            Knob::Niceness => "how pleasant the name's meaning is, whatever the topic",
        }
    }

    pub fn get(self, w: &Weights) -> f32 {
        match self {
            Knob::Name => w.name,
            Knob::Coverage => w.coverage,
            Knob::Usage => w.usage,
            Knob::Relation => w.relation,
            Knob::Niceness => w.niceness,
        }
    }

    fn set(self, w: &mut Weights, v: f32) {
        match self {
            Knob::Name => w.name = v,
            Knob::Coverage => w.coverage = v,
            Knob::Usage => w.usage = v,
            Knob::Relation => w.relation = v,
            Knob::Niceness => w.niceness = v,
        }
    }
}

/// What the availability column is showing, and why.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Snapshot {
    Missing,
    Downloading,
    Ready { crates: usize, age: Duration },
    Failed,
}

pub struct App {
    /// The keyword line, as typed. Parsed on the search debounce, not per key,
    /// so an unfinished bracket is not an error while it is being typed.
    pub query: String,
    /// Words steering what the name should *mean*.
    pub steer: String,
    /// Steer by the pattern's own words as well as the steering line.
    pub inherit: bool,
    pub focus: Focus,
    pub overlay: Overlay,
    pub menu_row: usize,
    /// Which button the exit dialog is on. Starts on `yes`: reaching it takes
    /// esc and a menu choice already, so the dialog is a last check rather
    /// than the thing standing between a stray key and the exit.
    pub exit_yes: bool,
    /// Whether the overlay on screen was opened from the menu, so esc knows
    /// whether to fall back to it or to the boxes.
    from_menu: bool,
    /// The last pattern that parsed, kept so an empty result set can say which
    /// relaxation would have helped.
    pattern: Pattern,
    pub parse_error: Option<ParseError>,
    pub config: Config,

    /// The full ranking, shared by every tab.
    //
    // a tab is the global ranking filtered by language -- ranking is by
    // absolute score, so filtering cannot reorder anything. Keeping one vector
    // and four index lists costs ~38 MB instead of four times that.
    results: Vec<Suggestion>,
    availability: Vec<Availability>,
    views: [Vec<u32>; 4],

    pub tab: usize,
    selected: [usize; 4],
    offset: [usize; 4],
    /// Rows the results pane can show, set by the renderer each frame.
    pub viewport: usize,
    /// Columns the expansion needs to cover all but the widest few rows.
    expansion_width: usize,

    pub only_available: bool,
    pub snapshot: Snapshot,
    pub status: String,
    pub searching: bool,
    /// Bumped per search so a slow reply cannot overwrite a newer one.
    pub generation: u64,
    pub should_quit: bool,
    pub option_row: usize,
    /// Columns the options grid is laid out in, set by the renderer each frame
    /// from the terminal width. Navigation needs it and cannot measure a
    /// terminal, so it is told.
    option_columns: usize,
    /// A number in the options box has been picked up, so up and down step it
    /// instead of leaving the box.
    pub adjusting: bool,
    pub knob_row: usize,
    recheck_requested: bool,
}

impl App {
    pub fn new(config: Config) -> App {
        App {
            query: String::new(),
            steer: String::new(),
            inherit: true,
            focus: Focus::Pattern,
            overlay: Overlay::None,
            menu_row: 0,
            exit_yes: true,
            from_menu: false,
            pattern: Pattern::default(),
            parse_error: None,
            config,
            results: Vec::new(),
            availability: Vec::new(),
            views: [const { Vec::new() }; 4],
            tab: 3,
            selected: [0; 4],
            offset: [0; 4],
            viewport: 20,
            expansion_width: 0,
            only_available: true,
            snapshot: Snapshot::Missing,
            status: String::new(),
            searching: false,
            generation: 0,
            should_quit: false,
            option_row: 0,
            option_columns: Opt::ORDER.len(),
            adjusting: false,
            knob_row: 0,
            recheck_requested: false,
        }
    }

    /// The line typing currently goes to, if any.
    fn line(&mut self) -> Option<&mut String> {
        match self.focus {
            Focus::Pattern => Some(&mut self.query),
            Focus::Steer => Some(&mut self.steer),
            _ => None,
        }
    }

    /// Type into the focused input line. Returns whether anything changed, so
    /// the caller knows whether to restart the search debounce.
    pub fn type_char(&mut self, c: char) -> bool {
        match self.line() {
            Some(l) => {
                l.push(c);
                true
            }
            None => false,
        }
    }

    pub fn backspace(&mut self) -> bool {
        match self.line() {
            Some(l) => l.pop().is_some(),
            None => false,
        }
    }

    pub fn next_box(&mut self) {
        self.set_focus(self.focus.next_box());
    }

    pub fn prev_box(&mut self) {
        self.set_focus(self.focus.prev_box());
    }

    fn set_focus(&mut self, to: Focus) {
        // a number left half-adjusted would keep stealing up and down after
        // the cursor had moved on
        self.adjusting = false;
        self.focus = to;
    }

    /// Open the esc menu.
    pub fn open_menu(&mut self) {
        self.overlay = Overlay::Menu;
        self.from_menu = false;
    }

    /// Open an overlay from a shortcut rather than from the menu, so esc
    /// returns to the boxes instead of somewhere the user never was.
    pub fn open_direct(&mut self, overlay: Overlay) {
        self.overlay = overlay;
        self.from_menu = false;
    }

    /// Open the exit dialog, from wherever it was asked for.
    //
    // leaves `from_menu` alone: asked for from the boxes it backs out to them,
    // asked for on top of a menu submenu it backs out to the menu
    pub fn open_exit(&mut self) {
        self.overlay = Overlay::Exit;
        self.exit_yes = true;
    }

    pub fn menu_move(&mut self, delta: isize) {
        let last = MenuItem::ORDER.len() as isize - 1;
        self.menu_row = (self.menu_row as isize + delta).clamp(0, last) as usize;
    }

    pub fn menu_item(&self) -> MenuItem {
        MenuItem::ORDER[self.menu_row.min(MenuItem::ORDER.len() - 1)]
    }

    /// Enter on a menu line: open what it names.
    pub fn menu_activate(&mut self) {
        self.overlay = self.menu_item().opens();
        self.from_menu = true;
        self.exit_yes = true;
    }

    /// Esc from an overlay: back to the menu if that is where it came from,
    /// otherwise back to the boxes.
    pub fn close_overlay(&mut self) {
        self.overlay = match self.overlay {
            Overlay::Menu | Overlay::None => Overlay::None,
            _ if self.from_menu => Overlay::Menu,
            _ => Overlay::None,
        };
        // `from_menu` describes the chain that is open, so closing the last of
        // it clears the flag rather than leaving it to mislead the next one
        if self.overlay == Overlay::None {
            self.from_menu = false;
        }
    }

    /// Move between `[yes]` and `[no]`.
    pub fn exit_move(&mut self, delta: isize) {
        if delta != 0 {
            self.exit_yes = delta < 0;
        }
    }

    /// Move up the screen. `Change::Search` when a number was stepped instead.
    pub fn focus_up(&mut self) -> Change {
        match self.focus {
            Focus::Pattern => Change::None,
            Focus::Steer => {
                self.set_focus(Focus::Pattern);
                Change::None
            }
            Focus::Options if self.adjusting => self.adjust_option(1),
            Focus::Options if self.option_row >= self.option_columns => {
                // the grid wraps, so up walks its rows before leaving the box
                self.option_row -= self.option_columns;
                Change::None
            }
            Focus::Options => {
                self.set_focus(Focus::Steer);
                Change::None
            }
            Focus::Results => {
                // the list is part of the same chain, so running off the top
                // of it lands on the options box rather than stopping dead
                if self.selected() == 0 {
                    self.set_focus(Focus::Options);
                } else {
                    self.move_selection(-1);
                }
                Change::None
            }
        }
    }

    pub fn focus_down(&mut self) -> Change {
        match self.focus {
            Focus::Pattern => {
                self.set_focus(Focus::Steer);
                Change::None
            }
            Focus::Steer => {
                self.set_focus(Focus::Options);
                Change::None
            }
            Focus::Options if self.adjusting => self.adjust_option(-1),
            Focus::Options if !self.on_last_option_row() => {
                // land on the last cell rather than skipping the box when the
                // row below is short -- 7 options over 4 columns leaves a gap
                self.option_row =
                    (self.option_row + self.option_columns).min(Opt::ORDER.len() - 1);
                Change::None
            }
            Focus::Options => {
                self.set_focus(Focus::Results);
                Change::None
            }
            Focus::Results => {
                self.move_selection(1);
                Change::None
            }
        }
    }

    /// How wide the options grid is. The renderer measures the terminal and
    /// says; navigation only has to agree with what was drawn.
    pub fn set_option_columns(&mut self, columns: usize) {
        self.option_columns = columns.max(1);
    }

    pub fn option_columns(&self) -> usize {
        self.option_columns
    }

    /// Rows the options grid needs at the current width.
    pub fn option_rows(&self) -> usize {
        Opt::ORDER.len().div_ceil(self.option_columns)
    }

    fn on_last_option_row(&self) -> bool {
        self.option_row / self.option_columns
            == (Opt::ORDER.len() - 1) / self.option_columns
    }

    /// Move left or right within the focused box.
    pub fn move_across(&mut self, delta: isize) {
        match self.focus {
            Focus::Options => {
                self.adjusting = false;
                let last = Opt::ORDER.len() as isize - 1;
                self.option_row =
                    (self.option_row as isize + delta).clamp(0, last) as usize;
            }
            Focus::Results => {
                if delta > 0 {
                    self.next_tab();
                } else {
                    self.prev_tab();
                }
            }
            // the input lines have no cursor to move yet
            Focus::Pattern | Focus::Steer => {}
        }
    }

    /// Enter: flip a switch, pick up a number, or leave a text line.
    pub fn activate(&mut self) -> Change {
        match self.focus {
            Focus::Pattern | Focus::Steer => {
                self.set_focus(Focus::Results);
                Change::None
            }
            Focus::Options => {
                let opt = self.option();
                if opt.is_toggle() {
                    self.toggle(opt)
                } else {
                    self.adjusting = !self.adjusting;
                    Change::None
                }
            }
            Focus::Results => Change::None,
        }
    }

    /// The option under the cursor.
    pub fn option(&self) -> Opt {
        Opt::ORDER[self.option_row.min(Opt::ORDER.len() - 1)]
    }

    /// Flip one switch, wherever the request came from.
    pub fn toggle(&mut self, opt: Opt) -> Change {
        match opt {
            Opt::Inherit => {
                self.inherit = !self.inherit;
                Change::Search
            }
            Opt::Recursive => {
                self.config.allow_recursion = !self.config.allow_recursion;
                Change::Search
            }
            Opt::OnlyAvailable => {
                self.only_available = !self.only_available;
                self.rebuild_views();
                Change::None
            }
            // stepping a number is adjust_option's job
            _ => Change::None,
        }
    }

    /// Whether a switch is on. Meaningless for the numbers.
    pub fn option_on(&self, opt: Opt) -> bool {
        match opt {
            Opt::Inherit => self.inherit,
            Opt::Recursive => self.config.allow_recursion,
            Opt::OnlyAvailable => self.only_available,
            _ => false,
        }
    }

    pub fn steering(&self) -> Steering {
        Steering::parse(&self.steer, self.inherit)
    }

    /// Read the keyword line and say what to do about it.
    //
    // the parse happens here rather than per keystroke so that a half-written
    // `(source|code` is simply not searched yet, instead of flashing an error
    // under the cursor as it is typed
    pub fn take_query(&mut self) -> Typed {
        match nomen::pattern::parse(&self.query) {
            Err(e) => {
                self.parse_error = Some(e);
                Typed::Invalid
            }
            Ok(p) => {
                self.parse_error = None;
                self.pattern = p.clone();
                if p.is_empty() { Typed::Clear } else { Typed::Search(p, self.steering()) }
            }
        }
    }

    /// What to say when there is nothing to show.
    ///
    /// Every block is a hard requirement, so an over-specified pattern finding
    /// nothing is the common failure — and the fix is always one character.
    /// Naming it beats a shrug.
    pub fn empty_hint(&self) -> Vec<String> {
        if let Some(e) = &self.parse_error {
            return vec![format!("{e}")];
        }
        if self.query.trim().is_empty() {
            return vec!["type keywords above".into()];
        }
        if self.searching {
            return vec!["searching…".into()];
        }

        if self.result_count() > 0 {
            // names were found; something downstream of the search hid them
            if !self.views[3].is_empty() {
                return vec!["no names in this language — try the 'all' tab".into()];
            }
            if self.only_available {
                return vec![
                    "every name found is already published on crates.io".into(),
                    "  ^a  show taken names too".into(),
                ];
            }
        }

        let p = &self.pattern;
        let mut out = vec!["no name spells this pattern.".into()];
        if !p.terms.iter().any(|t| t.groups.iter().any(|g| g.interior)) {
            out.push("  ~block   let a block use any of its letters".into());
        }
        if p.terms.len() > 1 && !p.terms.iter().any(|t| t.optional) {
            out.push("  ?block   let the name skip a block".into());
        }
        if p.terms.iter().any(|t| t.width() > 1) {
            out.push("  a-b      demands two adjacent letters".into());
        }
        if out.len() == 1 {
            out.push("  try a longer max length, or fewer blocks".into());
        }
        out
    }

    /// Accept a finished search, unless a newer one has already been asked for.
    pub fn accept(&mut self, generation: u64, results: Vec<Suggestion>) -> bool {
        if generation != self.generation {
            return false;
        }
        self.results = results;
        self.searching = false;
        self.expansion_width = measure_expansions(&self.results);
        self.recompute_availability();
        self.rebuild_views();
        true
    }

    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot;
        self.recompute_availability();
        self.rebuild_views();
    }

    /// Availability for every result, from whatever the snapshot knows.
    fn recompute_availability(&mut self) {
        let known = matches!(self.snapshot, Snapshot::Ready { .. });
        self.availability = vec![
            if known { Availability::Available } else { Availability::Unknown };
            self.results.len()
        ];
    }

    /// Set availability row by row. Test-only: the running app always has a
    /// snapshot to ask, and uses [`App::apply_availability_with`].
    #[cfg(test)]
    pub fn apply_availability(&mut self, statuses: Vec<Availability>) {
        if statuses.len() == self.results.len() {
            self.availability = statuses;
            self.rebuild_views();
        }
    }

    /// Recompute availability for every result from a lookup.
    //
    // takes a closure rather than a list of names: the ranking runs to ~77k
    // entries and cloning every name to ask about it is pure waste
    pub fn apply_availability_with(&mut self, status: impl Fn(&str) -> Availability) {
        self.availability = self.results.iter().map(|s| status(&s.name)).collect();
        self.rebuild_views();
    }

    /// Override one row after an authoritative re-check.
    pub fn set_row_availability(&mut self, index: usize, status: Availability) {
        if let Some(slot) = self.availability.get_mut(index) {
            *slot = status;
            self.rebuild_views();
        }
    }

    fn rebuild_views(&mut self) {
        for (i, tab) in Tab::ORDER.iter().enumerate() {
            let view: Vec<u32> = self
                .results
                .iter()
                .enumerate()
                .filter(|(j, s)| {
                    tab.accepts(s.lang)
                        && (!self.only_available
                            || self.availability.get(*j) == Some(&Availability::Available))
                })
                .map(|(j, _)| j as u32)
                .collect();
            self.views[i] = view;
            let len = self.views[i].len();
            self.selected[i] = self.selected[i].min(len.saturating_sub(1));
            if len == 0 {
                self.selected[i] = 0;
                self.offset[i] = 0;
            }
            self.clamp_offset(i);
        }
    }

    /// Step the number under the cursor. Switches are flipped by [`App::toggle`].
    pub fn adjust_option(&mut self, delta: isize) -> Change {
        let opt = self.option();
        let c = &mut self.config;
        match opt {
            Opt::MinLen => {
                let v = (c.min_len as isize + delta).clamp(1, c.max_len as isize) as usize;
                if v == c.min_len {
                    return Change::None;
                }
                c.min_len = v;
            }
            Opt::MaxLen => {
                // 8 is the corpus ceiling: nothing longer was ever extracted,
                // so raising this past it would silently find nothing new
                let v = (c.max_len as isize + delta).clamp(c.min_len as isize, 8) as usize;
                if v == c.max_len {
                    return Change::None;
                }
                c.max_len = v;
            }
            Opt::MaxGaps => c.max_gaps = step(c.max_gaps, delta, 8),
            Opt::MaxPerLemma => c.max_per_lemma = step(c.max_per_lemma, delta, 20),
            _ => return Change::None,
        }
        Change::Search
    }

    /// How an option currently reads. A switch reads as a tick box, so its
    /// state survives a terminal with no colour.
    pub fn option_value(&self, opt: Opt) -> String {
        let c = &self.config;
        let show = |v: Option<usize>| v.map_or("none".to_string(), |n| n.to_string());
        match opt {
            Opt::MinLen => c.min_len.to_string(),
            Opt::MaxLen => c.max_len.to_string(),
            Opt::MaxGaps => show(c.max_gaps),
            Opt::MaxPerLemma => show(c.max_per_lemma),
            _ => if self.option_on(opt) { "[x]".into() } else { "[ ]".into() },
        }
    }

    /// Nudge the weight under the weights cursor.
    pub fn adjust_knob(&mut self, delta: f32) -> Change {
        let knob = Knob::ORDER[self.knob_row.min(Knob::ORDER.len() - 1)];
        let now = knob.get(&self.config.weights);
        let next = (now + delta).clamp(0.0, 5.0);
        if (next - now).abs() < f32::EPSILON {
            return Change::None;
        }
        knob.set(&mut self.config.weights, next);
        Change::Resort
    }

    /// Re-rank the results already in hand under the current weights.
    //
    // weights only feed Score::total, so nothing has to be searched again --
    // which is what makes nudging a weight feel immediate.
    //
    // **note: the set being re-sorted is the one generate() already deduped by
    // name and capped per lemma, and those decisions were made under the old
    // weights. The names on offer are therefore stable across a nudge; which
    // inflected form represents a lemma can lag until the next real search.
    pub fn resort(&mut self) {
        let anchor = self.current().map(|(_, s, _)| s.name.clone());
        for s in &mut self.results {
            s.total = s.score.total(&self.config.weights);
        }
        let mut order: Vec<usize> = (0..self.results.len()).collect();
        order.sort_by(|&a, &b| self.results[b].total.total_cmp(&self.results[a].total));
        let mut reordered = Vec::with_capacity(self.results.len());
        let mut statuses = Vec::with_capacity(self.results.len());
        for i in order {
            reordered.push(std::mem::replace(&mut self.results[i], placeholder()));
            statuses.push(self.availability[i]);
        }
        self.results = reordered;
        self.availability = statuses;
        self.rebuild_views();

        // keep the cursor on the name the user was looking at
        if let Some(name) = anchor {
            let tab = self.tab;
            if let Some(pos) =
                self.views[tab].iter().position(|&i| self.results[i as usize].name == name)
            {
                self.selected[tab] = pos;
                self.clamp_offset(tab);
            }
        }
    }

    pub fn next_tab(&mut self) {
        self.tab = (self.tab + 1) % Tab::ORDER.len();
    }

    pub fn prev_tab(&mut self) {
        self.tab = (self.tab + Tab::ORDER.len() - 1) % Tab::ORDER.len();
    }

    pub fn view(&self) -> &[u32] {
        &self.views[self.tab]
    }

    pub fn selected(&self) -> usize {
        self.selected[self.tab]
    }

    pub fn offset(&self) -> usize {
        self.offset[self.tab]
    }

    /// The result under the cursor, and its index into the shared vector.
    pub fn current(&self) -> Option<(usize, &Suggestion, Availability)> {
        let idx = *self.view().get(self.selected())? as usize;
        Some((idx, self.results.get(idx)?, self.availability[idx]))
    }

    pub fn row(&self, view_index: usize) -> Option<(&Suggestion, Availability)> {
        let idx = *self.view().get(view_index)? as usize;
        Some((&self.results[idx], self.availability[idx]))
    }

    pub fn move_selection(&mut self, delta: isize) {
        let len = self.view().len();
        if len == 0 {
            return;
        }
        let cur = self.selected[self.tab] as isize;
        self.selected[self.tab] = cur.saturating_add(delta).clamp(0, len as isize - 1) as usize;
        self.clamp_offset(self.tab);
    }

    pub fn select_first(&mut self) {
        self.selected[self.tab] = 0;
        self.clamp_offset(self.tab);
    }

    pub fn select_last(&mut self) {
        self.selected[self.tab] = self.views[self.tab].len().saturating_sub(1);
        self.clamp_offset(self.tab);
    }

    /// Keep the cursor inside the visible window, scrolling only as far as it
    /// must — a recentring scroll makes long lists feel unsteady.
    fn clamp_offset(&mut self, tab: usize) {
        let sel = self.selected[tab];
        let h = self.viewport.max(1);
        let off = &mut self.offset[tab];
        if sel < *off {
            *off = sel;
        } else if sel >= *off + h {
            *off = sel + 1 - h;
        }
        let len = self.views[tab].len();
        *off = (*off).min(len.saturating_sub(h.min(len)));
    }

    pub fn set_viewport(&mut self, rows: usize) {
        if rows != self.viewport {
            self.viewport = rows.max(1);
            let tab = self.tab;
            self.clamp_offset(tab);
        }
    }

    /// How wide the expansion column has to be to hold all but the widest few
    /// rows. The renderer starts the gloss just past it.
    //
    // measured once per search rather than per frame: a column that moved as
    // the list scrolled would be worse than one that never lined up at all
    pub fn expansion_width(&self) -> usize {
        self.expansion_width
    }

    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    pub fn request_recheck(&mut self) {
        self.recheck_requested = true;
    }

    /// Take a pending re-check request, if one was made since the last call.
    pub fn take_recheck_request(&mut self) -> bool {
        std::mem::take(&mut self.recheck_requested)
    }

    /// How many results a given tab holds, for the tab bar.
    pub fn count_for(&self, tab: usize) -> usize {
        self.views.get(tab).map_or(0, |v| v.len())
    }
}

/// Share of glossed rows the expansion column is sized to hold.
//
// the rest run past it and take their own gloss with them. Pushing to 100%
// would let one freakishly long expansion shove every gloss on screen to the
// right; at 95 that row goes ragged alone.
const FIT: usize = 95;

/// A fallback for a result set with nothing to measure.
const DEFAULT_EXPANSION_WIDTH: usize = 32;

/// Columns the expansion of `s` will occupy: each word, or one letter for a
/// gap, plus the spaces between them.
fn expansion_width(s: &Suggestion) -> usize {
    let separators = s.expansion.len().saturating_sub(1);
    let words: usize = s
        .expansion
        .iter()
        .map(|slot| match slot {
            ExpandedSlot::Block { word, .. } | ExpandedSlot::SelfRef { word } => {
                word.chars().count()
            }
            ExpandedSlot::Gap => 1,
        })
        .sum();
    words + separators
}

/// The width that covers [`FIT`] percent of the rows that will show a gloss.
//
// only glossed rows are measured, because they are the only ones being lined
// up -- an English entry has nothing to put in that column. A histogram rather
// than a sort: expansions are at most a few dozen columns, so counting them is
// one pass and no allocation.
fn measure_expansions(results: &[Suggestion]) -> usize {
    const CAP: usize = 160;
    let mut hist = [0usize; CAP + 1];
    let mut total = 0;
    for s in results.iter().filter(|s| s.lang != Lang::English) {
        hist[expansion_width(s).min(CAP)] += 1;
        total += 1;
    }
    if total == 0 {
        return DEFAULT_EXPANSION_WIDTH;
    }

    let target = (total * FIT).div_ceil(100);
    let mut seen = 0;
    for (width, count) in hist.iter().enumerate() {
        seen += count;
        if seen >= target {
            return width;
        }
    }
    DEFAULT_EXPANSION_WIDTH
}

/// Step an optional count, where one below zero means "no limit".
fn step(value: Option<usize>, delta: isize, max: usize) -> Option<usize> {
    let current = value.map_or(-1, |v| v as isize);
    let next = (current + delta).clamp(-1, max as isize);
    (next >= 0).then_some(next as usize)
}

/// A cheap stand-in used while permuting the results vector.
fn placeholder() -> Suggestion {
    Suggestion {
        name: String::new(),
        display: String::new(),
        gloss: String::new(),
        lemma: String::new(),
        lang: Lang::English,
        expansion: Vec::new(),
        gaps: 0,
        score: nomen::Score::default(),
        total: f32::NEG_INFINITY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomen::{ExpandedSlot, Score};

    pub(super) fn suggestion(name: &str, lang: Lang) -> Suggestion {
        Suggestion {
            name: name.to_string(),
            display: name.to_string(),
            gloss: String::new(),
            lemma: name.to_string(),
            lang,
            expansion: vec![ExpandedSlot::Gap; name.len()],
            gaps: name.len(),
            score: Score::default(),
            total: 1.0,
        }
    }

    fn app_with(results: Vec<Suggestion>) -> App {
        let mut app = App::new(Config::default());
        app.only_available = false;
        app.snapshot = Snapshot::Ready { crates: 1, age: Duration::ZERO };
        app.accept(0, results);
        app
    }

    #[test]
    fn tabs_partition_the_ranking_by_language() {
        let app = app_with(vec![
            suggestion("aaa", Lang::English),
            suggestion("bbb", Lang::Latin),
            suggestion("ccc", Lang::Greek),
            suggestion("ddd", Lang::English),
        ]);
        let count = |t: Tab| {
            app.results.iter().filter(|s| t.accepts(s.lang)).count()
        };
        assert_eq!(app.views[0].len(), count(Tab::English));
        assert_eq!(app.views[1].len(), count(Tab::Latin));
        assert_eq!(app.views[2].len(), count(Tab::Greek));
        assert_eq!(app.views[3].len(), 4, "the all tab holds everything");
    }

    #[test]
    fn the_expansion_column_is_sized_to_all_but_the_widest_rows() {
        use nomen::ExpandedSlot;
        let wide = |name: &str, word: &str, lang| {
            let mut s = suggestion(name, lang);
            s.expansion = vec![ExpandedSlot::Block {
                word: word.to_string(),
                term: 0,
                group: 0,
                letter: 0,
            }];
            s
        };

        // ninety-five short rows and five long ones: the column holds the
        // short ones and lets the outliers run past it
        let mut rows: Vec<Suggestion> =
            (0..95).map(|i| wide(&format!("s{i}"), "abcde", Lang::Greek)).collect();
        rows.extend((0..5).map(|i| wide(&format!("l{i}"), &"x".repeat(60), Lang::Latin)));
        let mut a = App::new(Config::default());
        a.accept(0, rows);
        assert_eq!(a.expansion_width(), 5, "one long row must not shove every gloss right");

        // English rows have no gloss, so they are not what is being lined up
        let mut a = App::new(Config::default());
        a.accept(0, vec![wide("e", &"x".repeat(60), Lang::English)]);
        assert_eq!(a.expansion_width(), DEFAULT_EXPANSION_WIDTH);
    }

    #[test]
    fn a_stale_search_result_is_discarded() {
        let mut app = App::new(Config::default());
        app.generation = 7;
        assert!(!app.accept(6, vec![suggestion("old", Lang::English)]));
        assert_eq!(app.result_count(), 0);
        assert!(app.accept(7, vec![suggestion("new", Lang::English)]));
        assert_eq!(app.result_count(), 1);
    }

    #[test]
    fn selection_clamps_at_both_ends() {
        let mut app = app_with((0..5).map(|i| suggestion(&format!("w{i}"), Lang::English)).collect());
        app.move_selection(-10);
        assert_eq!(app.selected(), 0);
        app.move_selection(100);
        assert_eq!(app.selected(), 4);
        app.select_first();
        assert_eq!(app.selected(), 0);
    }

    #[test]
    fn the_window_follows_the_cursor_without_recentring() {
        let mut app = app_with((0..100).map(|i| suggestion(&format!("w{i}"), Lang::English)).collect());
        app.set_viewport(10);
        app.move_selection(9);
        assert_eq!(app.offset(), 0, "still inside the first window");
        app.move_selection(1);
        assert_eq!(app.offset(), 1, "scrolled by exactly one");
    }

    #[test]
    fn each_tab_keeps_its_own_cursor() {
        let mut app = app_with(vec![
            suggestion("aaa", Lang::English),
            suggestion("bbb", Lang::English),
            suggestion("ccc", Lang::Latin),
        ]);
        app.tab = 0;
        app.move_selection(1);
        app.tab = 1;
        assert_eq!(app.selected(), 0);
        app.tab = 0;
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn only_available_filters_every_tab() {
        let mut app = app_with(vec![
            suggestion("aaa", Lang::English),
            suggestion("bbb", Lang::English),
        ]);
        app.apply_availability(vec![Availability::Taken, Availability::Available]);
        assert_eq!(app.views[0].len(), 2);
        app.toggle(Opt::OnlyAvailable);
        assert_eq!(app.views[0].len(), 1);
        assert_eq!(app.row(0).unwrap().0.name, "bbb");
    }

    #[test]
    fn a_recheck_can_move_a_row_out_of_the_available_filter() {
        let mut app = app_with(vec![suggestion("aaa", Lang::English)]);
        app.apply_availability(vec![Availability::Available]);
        app.toggle(Opt::OnlyAvailable);
        assert_eq!(app.view().len(), 1);
        app.set_row_availability(0, Availability::Taken);
        assert_eq!(app.view().len(), 0, "the filter reruns after a re-check");
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;

    fn app() -> App {
        let mut a = App::new(Config::default());
        a.only_available = false;
        a
    }

    impl App {
        /// Put the options cursor on a named option, for tests that care which
        /// one it is rather than where it sits.
        fn hover(&mut self, opt: Opt) {
            self.option_row = Opt::ORDER.iter().position(|o| *o == opt).unwrap();
        }
    }

    #[test]
    fn every_help_row_fits_the_overlay() {
        // the overlay is a fixed width, so an over-long line would be silently
        // clipped against the border rather than wrapped
        let room = HELP_WIDTH as usize - 2 - 2 - HELP_KEY_WIDTH;
        for line in HELP {
            let HelpLine::Row(key, what) = line else { continue };
            let (key, what) = (*key, *what);
            assert!(key.chars().count() <= HELP_KEY_WIDTH, "{key:?} overruns the key column");
            assert!(what.chars().count() <= room, "{what:?} is {} over", what.chars().count() - room);
        }
    }

    #[test]
    fn every_switch_has_a_shortcut_that_finds_its_way_back() {
        for opt in Opt::ORDER {
            match opt.key() {
                Some(c) => {
                    assert!(opt.is_toggle(), "{opt:?} is a number and should have no shortcut");
                    assert_eq!(Opt::from_key(c), Some(opt), "^{c} does not lead back to {opt:?}");
                }
                None => assert!(!opt.is_toggle(), "{opt:?} is a switch with no shortcut"),
            }
        }
        assert_eq!(Opt::from_key('z'), None);
    }

    #[test]
    fn the_options_grid_reflows_and_the_arrows_follow_it() {
        let mut a = app();
        a.focus = Focus::Options;

        // four across: seven options make two rows, the second one short
        a.set_option_columns(4);
        assert_eq!(a.option_rows(), 2);
        a.option_row = 0;
        a.focus_down();
        assert_eq!((a.focus, a.option_row), (Focus::Options, 4), "down walks the grid");
        a.focus_up();
        assert_eq!(a.option_row, 0);
        a.focus_up();
        assert_eq!(a.focus, Focus::Steer, "and leaves the box from the top row");

        // from the ragged end of the top row, down lands on the last cell
        // rather than skipping the box
        a.focus = Focus::Options;
        a.option_row = 3;
        a.focus_down();
        assert_eq!((a.focus, a.option_row), (Focus::Options, 6));
        a.focus_down();
        assert_eq!(a.focus, Focus::Results, "the last row is the way out");

        // one across: every option is its own row
        a.focus = Focus::Options;
        a.set_option_columns(1);
        assert_eq!(a.option_rows(), Opt::ORDER.len());
        a.option_row = 0;
        for want in 1..Opt::ORDER.len() {
            a.focus_down();
            assert_eq!((a.focus, a.option_row), (Focus::Options, want));
        }
        a.focus_down();
        assert_eq!(a.focus, Focus::Results);
    }

    #[test]
    fn left_and_right_run_through_the_grid_across_its_rows() {
        let mut a = app();
        a.focus = Focus::Options;
        a.set_option_columns(4);
        a.option_row = 3;
        a.move_across(1);
        assert_eq!(a.option_row, 4, "right wraps onto the next row");
        a.move_across(-1);
        assert_eq!(a.option_row, 3);
        a.option_row = 0;
        a.move_across(-1);
        assert_eq!(a.option_row, 0, "and stops at the ends");
    }

    #[test]
    fn esc_opens_a_menu_and_every_entry_leads_back_to_it() {
        let mut a = app();
        a.open_menu();
        assert_eq!(a.overlay, Overlay::Menu);

        for (row, opens) in
            [(0, Overlay::Help), (1, Overlay::Weights), (2, Overlay::Exit)]
        {
            a.open_menu();
            a.menu_row = row;
            a.menu_activate();
            assert_eq!(a.overlay, opens, "menu row {row}");
            a.close_overlay();
            assert_eq!(a.overlay, Overlay::Menu, "esc returns to the menu it came from");
        }
        a.close_overlay();
        assert_eq!(a.overlay, Overlay::None, "and esc again closes it");
    }

    #[test]
    fn a_shortcut_overlay_backs_out_to_the_boxes_not_to_a_menu_never_opened() {
        let mut a = app();
        a.open_direct(Overlay::Weights);
        a.close_overlay();
        assert_eq!(a.overlay, Overlay::None);
    }

    #[test]
    fn the_exit_shortcut_backs_out_to_wherever_it_was_pressed() {
        let mut a = app();

        // from the boxes, with a menu visited and left earlier: esc must not
        // resurrect a menu that is no longer open
        a.open_menu();
        a.menu_activate();
        a.close_overlay();
        a.close_overlay();
        assert_eq!(a.overlay, Overlay::None);

        a.open_exit();
        assert!(a.exit_yes, "the shortcut arms the dialog like the menu does");
        a.close_overlay();
        assert_eq!(a.overlay, Overlay::None, "back to the boxes");

        // pressed on top of a submenu, it backs out to the menu it came from
        a.open_menu();
        a.menu_row = MenuItem::ORDER.iter().position(|m| *m == MenuItem::Help).unwrap();
        a.menu_activate();
        a.open_exit();
        a.close_overlay();
        assert_eq!(a.overlay, Overlay::Menu);
    }

    #[test]
    fn the_exit_dialog_starts_on_yes() {
        let mut a = app();
        a.open_menu();
        a.menu_row = MenuItem::ORDER.iter().position(|m| *m == MenuItem::Exit).unwrap();
        a.menu_activate();
        assert!(a.exit_yes, "getting here was deliberate enough");
        a.exit_move(1);
        assert!(!a.exit_yes, "right is no");
        a.exit_move(-1);
        assert!(a.exit_yes);
    }

    #[test]
    fn every_menu_line_says_what_it_opens() {
        for item in MenuItem::ORDER {
            assert!(!item.label().is_empty());
            assert!(!item.note().is_empty(), "{item:?} has no note");
        }
    }

    #[test]
    fn a_switch_reads_as_a_tick_box_so_colour_is_never_load_bearing() {
        let mut a = app();
        a.only_available = false;
        assert_eq!(a.option_value(Opt::OnlyAvailable), "[ ]");
        a.toggle(Opt::OnlyAvailable);
        assert_eq!(a.option_value(Opt::OnlyAvailable), "[x]");
    }

    #[test]
    fn every_option_and_weight_explains_itself() {
        // adding a variant without its help text should fail here rather than
        // ship a blank status line
        for o in Opt::ORDER {
            assert!(!o.description().is_empty(), "{o:?} has no description");
        }
        for k in Knob::ORDER {
            assert!(!k.description().is_empty(), "{k:?} has no description");
            assert!(!k.example().is_empty(), "{k:?} has no example");
            assert!(
                k.example().iter().all(|l| l.chars().count() <= 30),
                "{k:?} has an example too wide for the panel"
            );
        }
    }

    #[test]
    fn typing_goes_to_whichever_line_has_focus_and_nowhere_else() {
        let mut a = app();
        a.type_char('x');
        a.focus_down();
        a.type_char('y');
        a.focus_up();
        a.type_char('z');
        assert_eq!(a.query, "xz");
        assert_eq!(a.steer, "y");
        a.backspace();
        assert_eq!(a.query, "x", "backspace follows focus too");

        // outside the input lines a keystroke is not text, and saying so is
        // what lets main.rs skip the search debounce
        a.focus = Focus::Results;
        assert!(!a.type_char('q'));
        assert!(!a.backspace());
        assert_eq!(a.query, "x");
    }

    #[test]
    fn a_half_written_pattern_leaves_the_results_alone() {
        let mut a = app();
        a.query = "(source|code".into();
        assert!(matches!(a.take_query(), Typed::Invalid));
        assert!(a.parse_error.is_some());

        a.query.push(')');
        assert!(matches!(a.take_query(), Typed::Search(..)));
        assert!(a.parse_error.is_none());

        a.query.clear();
        assert!(matches!(a.take_query(), Typed::Clear));
    }

    #[test]
    fn steering_reflects_the_line_and_the_inherit_flag() {
        let mut a = app();
        a.steer = "graph theory".into();
        assert_eq!(a.steering().words, ["graph", "theory"]);
        assert!(a.steering().inherit, "inheriting is the default");
        a.toggle(Opt::Inherit);
        assert!(!a.steering().inherit);
    }

    #[test]
    fn an_empty_result_set_names_the_relaxation_that_would_help() {
        let mut a = app();
        a.query = "source code graph".into();
        a.take_query();
        let hint = a.empty_hint().join("\n");
        assert!(hint.contains('~'), "the load-bearing relaxation goes unmentioned: {hint}");
        assert!(hint.contains('?'), "the other one goes unmentioned: {hint}");

        // once both are already used, there is nothing left to suggest and the
        // hint must not pretend otherwise
        a.query = "~?source ~?code".into();
        a.take_query();
        let hint = a.empty_hint().join("\n");
        assert!(!hint.contains("~block") && !hint.contains("?block"), "stale advice: {hint}");

        a.query = "source-code".into();
        a.take_query();
        assert!(a.empty_hint().join("\n").contains("adjacent"));
    }

    #[test]
    fn a_parse_error_is_what_the_empty_screen_shows() {
        let mut a = app();
        a.query = "(source".into();
        a.take_query();
        assert!(a.empty_hint()[0].contains("unclosed"));
    }

    #[test]
    fn structural_options_ask_for_a_new_search_and_weights_do_not() {
        let mut a = app();
        a.hover(Opt::MaxGaps);
        assert_eq!(a.adjust_option(1), Change::Search);
        assert_eq!(a.adjust_knob(0.1), Change::Resort);
    }

    #[test]
    fn the_arrows_walk_the_whole_screen_from_top_to_bottom() {
        let mut a = app();
        a.accept(0, (0..5).map(|i| super::tests::suggestion(&format!("w{i}"), Lang::English)).collect());
        assert_eq!(a.focus, Focus::Pattern);

        for want in [Focus::Steer, Focus::Options, Focus::Results] {
            a.focus_down();
            assert_eq!(a.focus, want);
        }
        // inside the results the same key moves the selection
        a.focus_down();
        assert_eq!((a.focus, a.selected()), (Focus::Results, 1));

        // and up walks back out of the list once it is at the top
        a.focus_up();
        assert_eq!((a.focus, a.selected()), (Focus::Results, 0));
        for want in [Focus::Options, Focus::Steer, Focus::Pattern] {
            a.focus_up();
            assert_eq!(a.focus, want);
        }
        a.focus_up();
        assert_eq!(a.focus, Focus::Pattern, "the top of the screen is the end of the chain");
    }

    #[test]
    fn tab_jumps_whole_boxes_rather_than_rows() {
        let mut a = app();
        assert_eq!(a.focus.box_(), Box_::Search);
        a.next_box();
        assert_eq!(a.focus, Focus::Options);
        a.next_box();
        assert_eq!(a.focus, Focus::Results);
        a.next_box();
        assert_eq!(a.focus, Focus::Pattern, "and wraps back to the top");
        a.prev_box();
        assert_eq!(a.focus, Focus::Results);
    }

    #[test]
    fn enter_flips_a_switch_and_picks_up_a_number() {
        let mut a = app();
        a.focus = Focus::Options;

        a.hover(Opt::Recursive);
        let was = a.config.allow_recursion;
        assert_eq!(a.activate(), Change::Search);
        assert_ne!(a.config.allow_recursion, was, "a switch flips outright");
        assert!(!a.adjusting, "and is never picked up");

        a.hover(Opt::MinLen);
        a.activate();
        assert!(a.adjusting, "a number is picked up instead");

        // once picked up, up and down step it rather than leaving the box
        let before = a.config.min_len;
        assert_eq!(a.focus_up(), Change::Search);
        assert_eq!(a.config.min_len, before + 1);
        assert_eq!(a.focus, Focus::Options, "and the cursor stays put");

        a.activate();
        assert!(!a.adjusting);
        a.focus_up();
        assert_eq!(a.focus, Focus::Steer, "put down, up leaves the box again");
    }

    #[test]
    fn moving_the_cursor_puts_down_a_number_it_was_holding() {
        let mut a = app();
        a.focus = Focus::Options;
        a.hover(Opt::MaxGaps);
        a.activate();
        assert!(a.adjusting);
        a.move_across(-1);
        assert!(!a.adjusting, "a half-adjusted number would keep stealing up and down");
    }

    #[test]
    fn left_and_right_cycle_the_language_tabs_from_the_results() {
        let mut a = app();
        a.focus = Focus::Results;
        let was = a.tab;
        a.move_across(1);
        assert_ne!(a.tab, was);
        a.move_across(-1);
        assert_eq!(a.tab, was);
    }

    #[test]
    fn an_optional_count_steps_down_into_no_limit() {
        // one below zero is "none", which is how max-gaps and per-lemma are
        // switched off without a separate key
        assert_eq!(step(Some(1), -1, 8), Some(0));
        assert_eq!(step(Some(0), -1, 8), None);
        assert_eq!(step(None, -1, 8), None);
        assert_eq!(step(None, 1, 8), Some(0));
        assert_eq!(step(Some(8), 1, 8), Some(8));
    }

    #[test]
    fn lengths_cannot_cross_each_other() {
        let mut a = app();
        a.config.min_len = 5;
        a.config.max_len = 5;
        a.option_row = Opt::ORDER.iter().position(|o| *o == Opt::MinLen).unwrap();
        assert_eq!(a.adjust_option(1), Change::None, "min cannot pass max");
        a.option_row = Opt::ORDER.iter().position(|o| *o == Opt::MaxLen).unwrap();
        assert_eq!(a.adjust_option(-1), Change::None, "max cannot pass min");
    }

    #[test]
    fn a_weight_change_reorders_the_results_already_in_hand() {
        use nomen::{ExpandedSlot, Lang, Score, Suggestion};
        let mk = |name: &str, relation: f32, niceness: f32| Suggestion {
            name: name.into(),
            display: name.into(),
            gloss: String::new(),
            lemma: name.into(),
            lang: Lang::English,
            expansion: vec![ExpandedSlot::Gap; name.len()],
            gaps: 0,
            score: Score { relation, niceness, ..Score::default() },
            total: 0.0,
        };
        let mut a = App::new(Config::default());
        a.only_available = false;
        a.snapshot = Snapshot::Ready { crates: 1, age: std::time::Duration::ZERO };
        // "apt" wins on relation, "nice" wins on niceness
        a.accept(0, vec![mk("apt", 1.0, 0.0), mk("nice", 0.0, 1.0)]);

        a.config.weights = Weights { relation: 1.0, niceness: 0.0, ..Weights::default() };
        a.resort();
        assert_eq!(a.row(0).unwrap().0.name, "apt");

        // flipping which component matters must flip the ranking, with no
        // new search: the same two results are simply re-scored
        a.config.weights = Weights { relation: 0.0, niceness: 1.0, ..Weights::default() };
        a.resort();
        assert_eq!(a.row(0).unwrap().0.name, "nice");
        assert_eq!(a.result_count(), 2, "resorting invents and drops nothing");
    }

    #[test]
    fn the_cursor_stays_on_the_same_name_across_a_resort() {
        use nomen::{ExpandedSlot, Lang, Score, Suggestion};
        let mk = |name: &str, relation: f32| Suggestion {
            name: name.into(),
            display: name.into(),
            gloss: String::new(),
            lemma: name.into(),
            lang: Lang::English,
            expansion: vec![ExpandedSlot::Gap; name.len()],
            gaps: 0,
            score: Score { relation, ..Score::default() },
            total: 0.0,
        };
        let mut a = App::new(Config::default());
        a.only_available = false;
        a.snapshot = Snapshot::Ready { crates: 1, age: std::time::Duration::ZERO };
        a.accept(0, vec![mk("aaa", 0.9), mk("bbb", 0.5), mk("ccc", 0.1)]);
        a.config.weights = Weights { relation: 1.0, ..Weights::default() };
        a.resort();
        a.move_selection(2);
        let watched = a.current().unwrap().1.name.clone();
        a.config.weights = Weights { relation: -1.0, ..Weights::default() };
        a.resort();
        assert_eq!(a.current().unwrap().1.name, watched, "the cursor followed its row");
    }

    #[test]
    fn weights_clamp_and_stop_reporting_change_at_the_limit() {
        let mut a = app();
        a.knob_row = Knob::ORDER.iter().position(|k| *k == Knob::Relation).unwrap();
        for _ in 0..200 {
            a.adjust_knob(0.05);
        }
        assert_eq!(Knob::Relation.get(&a.config.weights), 5.0);
        assert_eq!(a.adjust_knob(0.05), Change::None, "already at the ceiling");
    }
}
