use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::{Datelike, Days, Local, NaiveDate};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;

use crate::api::auth::{self, Pkce, School, Session};
use crate::api::browser::{self, Method};
use crate::api::client::KretaClient;
use crate::api::models::{self, Absence, Exam, Grade, Homework, Lesson, Note, Student};
use crate::stats::{self, SubjectStats};
use crate::store::{self, RawData};

pub enum Event {
    Key(KeyEvent),
    Resize,
    Tick,
    Msg(Msg),
}

pub enum Msg {
    LoggedIn(Result<Session, String>),
    BrowserCode(Result<String, String>),
    Schools(String, Result<Vec<School>, String>),
    Fetched(Kind, Result<Value, String>),
    Week(NaiveDate, Result<Value, String>),
}

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Student,
    Grades,
    Absences,
    Exams,
    Homework,
    Notes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Grades,
    Timetable,
    Absences,
    Tasks,
    Stats,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Overview, Tab::Grades, Tab::Timetable, Tab::Absences, Tab::Tasks, Tab::Stats];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Áttekintés",
            Tab::Grades => "Jegyek",
            Tab::Timetable => "Órarend",
            Tab::Absences => "Hiányzások",
            Tab::Tasks => "Teendők",
            Tab::Stats => "Statisztika",
        }
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap()
    }
}

pub struct LoginForm {
    /// School search query, username, password.
    pub fields: [String; 3],
    pub focus: usize,
    pub school: Option<School>,
    pub results: Vec<School>,
    pub result_idx: usize,
    pub searching: bool,
    /// Query of the last search sent, and when the query field was last edited.
    pub query_sent: String,
    edited_at: Option<Instant>,
    pub browser: Option<BrowserLogin>,
    pub error: Option<String>,
    pub busy: bool,
}

pub struct BrowserLogin {
    pub verifier: String,
    pub url: String,
    pub input: String,
    pub method: Method,
    cancel: Arc<AtomicBool>,
}

impl Drop for BrowserLogin {
    /// Leaving browser mode (or the login screen) stops the background watcher.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Everything the UI renders, derived from `RawData`.
#[derive(Default)]
pub struct Data {
    pub student: Student,
    pub grades: Vec<Grade>,
    pub subjects: Vec<SubjectStats>,
    pub absences: Vec<Absence>,
    pub exams: Vec<Exam>,
    pub homework: Vec<Homework>,
    pub notes: Vec<Note>,
    /// Keyed by the Monday of the week.
    pub weeks: BTreeMap<NaiveDate, Vec<Lesson>>,
}

impl Data {
    fn from_raw(raw: &RawData) -> Self {
        let grades = models::grades(&raw.grades);
        Self {
            student: Student::from_json(&raw.student),
            subjects: stats::subjects(&grades),
            grades,
            absences: models::absences(&raw.absences),
            exams: models::exams(&raw.exams),
            homework: models::homework(&raw.homework),
            notes: models::notes(&raw.notes),
            weeks: raw.timetable.iter().filter_map(|(k, v)| Some((k.parse().ok()?, models::lessons(v)))).collect(),
        }
    }

    /// Lessons on the first day from today onward that has any (within loaded weeks).
    pub fn next_school_day(&self) -> Option<(NaiveDate, Vec<&Lesson>)> {
        let today = Local::now().date_naive();
        let mut by_day: BTreeMap<NaiveDate, Vec<&Lesson>> = BTreeMap::new();
        for l in self.weeks.values().flatten() {
            let d = l.start.date_naive();
            if d >= today {
                by_day.entry(d).or_default().push(l);
            }
        }
        by_day.into_iter().next()
    }
}

pub enum Screen {
    Login(Box<LoginForm>),
    Main,
}

pub struct App {
    pub screen: Screen,
    pub tab: Tab,
    pub raw: RawData,
    pub data: Data,
    pub client: Option<KretaClient>,
    pub demo: bool,
    tx: UnboundedSender<Event>,
    pub pending: usize,
    pub status: Option<(String, bool, Instant)>,
    pub show_help: bool,
    pub confirm_logout: bool,
    pub quit: bool,
    pub tick: u64,

    pub subject_idx: usize,
    pub grade_idx: usize,
    pub grades_focus_right: bool,
    pub simulating: bool,
    pub sim_weight: f64,
    pub sim: HashMap<String, Vec<(u8, f64)>>,

    pub week: NaiveDate,
    pub day_idx: usize,
    pub lesson_row: usize,

    pub absence_idx: usize,
    pub tasks_scroll: u16,
}

pub fn monday_of(d: NaiveDate) -> NaiveDate {
    d - Days::new(d.weekday().num_days_from_monday() as u64)
}

impl App {
    pub fn new(tx: UnboundedSender<Event>, demo: bool) -> Self {
        let today = Local::now().date_naive();
        let (screen, raw, client) = if demo {
            (Screen::Main, crate::demo::data(), None)
        } else if let Some(session) = store::load_session() {
            (Screen::Main, store::load_cache(), Some(KretaClient::new(session)))
        } else {
            (Screen::Login(Box::new(Self::empty_login())), RawData::default(), None)
        };
        let mut app = Self {
            screen,
            tab: Tab::Overview,
            data: Data::from_raw(&raw),
            raw,
            client,
            demo,
            tx,
            pending: 0,
            status: None,
            show_help: false,
            confirm_logout: false,
            quit: false,
            tick: 0,
            subject_idx: 0,
            grade_idx: 0,
            grades_focus_right: false,
            simulating: false,
            sim_weight: 100.0,
            sim: HashMap::new(),
            week: monday_of(today),
            day_idx: (today.weekday().num_days_from_monday() as usize).min(4),
            lesson_row: 0,
            absence_idx: 0,
            tasks_scroll: 0,
        };
        if app.client.is_some() {
            app.refresh();
        }
        app
    }

    pub fn empty_login() -> LoginForm {
        let school = store::load_school();
        LoginForm {
            fields: [school.as_ref().map(|s| s.name.clone()).unwrap_or_default(), String::new(), String::new()],
            focus: 0,
            query_sent: school.as_ref().map(|s| s.name.clone()).unwrap_or_default(),
            school,
            results: Vec::new(),
            result_idx: 0,
            searching: false,
            edited_at: None,
            browser: None,
            error: None,
            busy: false,
        }
    }

    /// Debounced school search while typing in the login form.
    pub fn on_tick(&mut self) {
        let Screen::Login(form) = &mut self.screen else { return };
        let Some(at) = form.edited_at else { return };
        if at.elapsed().as_millis() < 350 {
            return;
        }
        form.edited_at = None;
        let query = form.fields[0].trim().to_owned();
        if query.chars().count() < 3 {
            form.results.clear();
            form.searching = false;
            return;
        }
        if query == form.query_sent {
            return;
        }
        form.query_sent = query.clone();
        form.searching = true;
        self.spawn(async move {
            let res = auth::search_schools(&query).await.map_err(|e| format!("{e:#}"));
            Msg::Schools(query, res)
        });
    }

    pub fn set_status(&mut self, msg: impl Into<String>, error: bool) {
        self.status = Some((msg.into(), error, Instant::now()));
    }

    pub fn loading(&self) -> bool {
        self.pending > 0
    }

    // ---- background work -------------------------------------------------

    fn spawn<F>(&mut self, fut: F)
    where
        F: Future<Output = Msg> + Send + 'static,
    {
        self.pending += 1;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(Event::Msg(fut.await));
        });
    }

    pub fn refresh(&mut self) {
        let Some(c) = self.client.clone() else {
            if self.demo {
                self.set_status("Demó mód – nincs mit frissíteni", false);
            }
            return;
        };
        let since = Local::now().date_naive() - Days::new(14);
        for kind in [Kind::Student, Kind::Grades, Kind::Absences, Kind::Exams, Kind::Homework, Kind::Notes] {
            let c = c.clone();
            self.spawn(async move {
                let res = match kind {
                    Kind::Student => c.student().await,
                    Kind::Grades => c.grades().await,
                    Kind::Absences => c.absences().await,
                    Kind::Exams => c.exams().await,
                    Kind::Homework => c.homework(since).await,
                    Kind::Notes => c.notes().await,
                };
                Msg::Fetched(kind, res.map_err(|e| format!("{e:#}")))
            });
        }
        let this_week = monday_of(Local::now().date_naive());
        let mut weeks = vec![this_week, this_week + Days::new(7)];
        if !weeks.contains(&self.week) {
            weeks.push(self.week);
        }
        for w in weeks {
            self.fetch_week(w);
        }
    }

    fn fetch_week(&mut self, monday: NaiveDate) {
        let Some(c) = self.client.clone() else { return };
        self.spawn(async move {
            let res = c.timetable(monday, monday + Days::new(7)).await;
            Msg::Week(monday, res.map_err(|e| format!("{e:#}")))
        });
    }

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::LoggedIn(res) => {
                self.pending = self.pending.saturating_sub(1);
                match res {
                    Ok(session) => {
                        if let Err(e) = store::save_session(&session) {
                            self.set_status(format!("A munkamenet mentése nem sikerült: {e}"), true);
                        }
                        self.client = Some(KretaClient::new(session));
                        self.screen = Screen::Main;
                        self.refresh();
                    }
                    Err(e) => {
                        if let Screen::Login(f) = &mut self.screen {
                            f.busy = false;
                            f.browser = None;
                            f.error = Some(e);
                        }
                    }
                }
                return;
            }
            Msg::BrowserCode(res) => {
                self.pending = self.pending.saturating_sub(1);
                let Screen::Login(form) = &mut self.screen else { return };
                let Some(b) = &form.browser else { return };
                if b.cancel.load(Ordering::Relaxed) {
                    return;
                }
                match res {
                    Ok(code) => {
                        let verifier = b.verifier.clone();
                        form.busy = true;
                        form.error = None;
                        self.spawn(async move {
                            Msg::LoggedIn(auth::exchange_code(&code, &verifier).await.map_err(|e| format!("{e:#}")))
                        });
                    }
                    Err(e) => form.error = Some(e),
                }
                return;
            }
            Msg::Schools(query, res) => {
                self.pending = self.pending.saturating_sub(1);
                let Screen::Login(form) = &mut self.screen else { return };
                if form.fields[0].trim() != query {
                    return; // stale
                }
                form.searching = false;
                match res {
                    Ok(list) => {
                        form.result_idx = 0;
                        if list.len() == 1 {
                            Self::pick_school(form, list[0].clone());
                        } else {
                            form.results = list;
                        }
                    }
                    Err(e) => form.error = Some(format!("Iskolakeresés: {e}")),
                }
                return;
            }
            Msg::Fetched(kind, res) => match res {
                Ok(v) => {
                    let slot = match kind {
                        Kind::Student => &mut self.raw.student,
                        Kind::Grades => &mut self.raw.grades,
                        Kind::Absences => &mut self.raw.absences,
                        Kind::Exams => &mut self.raw.exams,
                        Kind::Homework => &mut self.raw.homework,
                        Kind::Notes => &mut self.raw.notes,
                    };
                    *slot = v;
                }
                Err(e) => self.set_status(format!("Hiba: {e}"), true),
            },
            Msg::Week(monday, res) => match res {
                Ok(v) => {
                    self.raw.timetable.insert(monday.to_string(), v);
                }
                Err(e) => self.set_status(format!("Órarend: {e}"), true),
            },
        }
        self.pending = self.pending.saturating_sub(1);
        self.data = Data::from_raw(&self.raw);
        self.clamp_selection();
        if self.pending == 0 {
            self.raw.fetched_at = Some(Local::now());
            if let Err(e) = store::save_cache(&self.raw) {
                self.set_status(format!("Gyorsítótár mentése sikertelen: {e}"), true);
            } else if !self.status.as_ref().is_some_and(|s| s.1 && s.2.elapsed().as_secs() < 5) {
                self.set_status("Frissítve", false);
            }
        }
    }

    fn clamp_selection(&mut self) {
        self.subject_idx = self.subject_idx.min(self.data.subjects.len().saturating_sub(1));
        self.absence_idx = self.absence_idx.min(self.data.absences.len().saturating_sub(1));
    }

    // ---- input -----------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if matches!(self.screen, Screen::Login(_)) {
            self.login_key(key);
            return;
        }
        if self.show_help {
            self.show_help = false;
            return;
        }
        if self.confirm_logout {
            self.confirm_logout = false;
            if matches!(key.code, KeyCode::Char('i' | 'I' | 'y' | 'Y') | KeyCode::Enter) {
                self.logout();
            }
            return;
        }
        if self.simulating {
            self.sim_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('L') => self.confirm_logout = true,
            KeyCode::Tab => self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()],
            KeyCode::BackTab => self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()],
            KeyCode::Char(c @ '1'..='6') => self.tab = Tab::ALL[c as usize - '1' as usize],
            _ => match self.tab {
                Tab::Grades => self.grades_key(key),
                Tab::Timetable => self.timetable_key(key),
                Tab::Absences => self.absences_key(key),
                Tab::Tasks => self.tasks_key(key),
                Tab::Overview | Tab::Stats => {}
            },
        }
    }

    fn login_key(&mut self, key: KeyEvent) {
        let Screen::Login(form) = &mut self.screen else { return };
        if form.busy {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('b') {
            if form.browser.take().is_none() {
                self.start_browser_login();
            }
            return;
        }
        if let Some(b) = &mut form.browser {
            match key.code {
                KeyCode::Esc => form.browser = None,
                KeyCode::Backspace => {
                    b.input.pop();
                }
                KeyCode::Char('u') if ctrl => b.input.clear(),
                KeyCode::Char(c) if !ctrl => b.input.push(c),
                KeyCode::Enter => {
                    let Some(code) = auth::code_from_redirect(b.input.trim()) else {
                        form.error = Some("Nem található kód a beillesztett címben".into());
                        return;
                    };
                    let verifier = b.verifier.clone();
                    b.cancel.store(true, Ordering::Relaxed);
                    form.busy = true;
                    form.error = None;
                    self.spawn(async move {
                        Msg::LoggedIn(auth::exchange_code(&code, &verifier).await.map_err(|e| format!("{e:#}")))
                    });
                }
                _ => {}
            }
            return;
        }

        // Focus 0 is the "log in with browser" button, 1..=3 the password form fields.
        let field = form.focus.checked_sub(1);
        let dropdown = form.focus == 1 && form.school.is_none() && !form.results.is_empty();
        if form.focus == 1 && matches!(key.code, KeyCode::Backspace | KeyCode::Char(_)) {
            form.school = None;
            form.results.clear();
            form.edited_at = Some(Instant::now());
        }
        match key.code {
            KeyCode::Esc if dropdown => form.results.clear(),
            KeyCode::Esc => self.quit = true,
            KeyCode::Down if dropdown => form.result_idx = (form.result_idx + 1).min(form.results.len() - 1),
            KeyCode::Up if dropdown => form.result_idx = form.result_idx.saturating_sub(1),
            KeyCode::Tab | KeyCode::Enter if dropdown => {
                let s = form.results[form.result_idx].clone();
                Self::pick_school(form, s);
                form.focus = 2;
            }
            KeyCode::Enter if form.focus == 0 => self.start_browser_login(),
            KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => form.focus = (form.focus + 3) % 4,
            KeyCode::Backspace => {
                if let Some(i) = field {
                    form.fields[i].pop();
                }
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(i) = field {
                    form.fields[i].clear();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(i) = field {
                    form.fields[i].push(c);
                }
            }
            KeyCode::Enter if form.focus < 3 => form.focus += 1,
            KeyCode::Enter => {
                let [_, user, pass] = form.fields.clone().map(|s| s.trim().to_owned());
                let Some(school) = form.school.clone() else {
                    form.error = Some("Válaszd ki az iskolát (OM azonosító vagy név alapján)".into());
                    form.focus = 1;
                    return;
                };
                if user.is_empty() || pass.is_empty() {
                    form.error = Some("Minden mezőt ki kell tölteni".into());
                    return;
                }
                let _ = store::save_school(&school);
                let inst = school.code;
                form.busy = true;
                form.error = None;
                self.spawn(async move {
                    Msg::LoggedIn(auth::login(&inst.to_lowercase(), &user, &pass).await.map_err(|e| format!("{e:#}")))
                });
            }
            _ => {}
        }
    }

    /// Open the login page in a browser and wait for the OAuth code in the background.
    fn start_browser_login(&mut self) {
        let Screen::Login(form) = &mut self.screen else { return };
        let pkce = Pkce::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let (url, watcher_cancel) = (pkce.authorize_url.clone(), cancel.clone());
        form.error = None;
        form.browser = Some(BrowserLogin {
            verifier: pkce.verifier,
            url: pkce.authorize_url,
            input: String::new(),
            method: browser::method(),
            cancel,
        });
        self.spawn(
            async move { Msg::BrowserCode(browser::login(url, watcher_cancel).await.map_err(|e| format!("{e:#}"))) },
        );
    }

    fn pick_school(form: &mut LoginForm, school: School) {
        form.fields[0] = school.name.clone();
        form.query_sent = school.name.clone();
        form.results.clear();
        form.school = Some(school);
        form.error = None;
    }

    fn logout(&mut self) {
        if self.demo {
            self.quit = true;
            return;
        }
        if let Some(c) = self.client.take() {
            tokio::spawn(async move { auth::revoke(&c.session().await).await });
        }
        store::clear();
        self.raw = RawData::default();
        self.data = Data::default();
        self.screen = Screen::Login(Box::new(Self::empty_login()));
    }

    fn grades_key(&mut self, key: KeyEvent) {
        let n_subjects = self.data.subjects.len();
        let n_grades = self.selected_subject().map_or(0, |s| s.grades.len());
        match key.code {
            KeyCode::Char('j') | KeyCode::Down if self.grades_focus_right => {
                self.grade_idx = (self.grade_idx + 1).min(n_grades.saturating_sub(1))
            }
            KeyCode::Char('k') | KeyCode::Up if self.grades_focus_right => {
                self.grade_idx = self.grade_idx.saturating_sub(1)
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.subject_idx = (self.subject_idx + 1).min(n_subjects.saturating_sub(1));
                self.grade_idx = 0;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.subject_idx = self.subject_idx.saturating_sub(1);
                self.grade_idx = 0;
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => self.grades_focus_right = true,
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => self.grades_focus_right = false,
            KeyCode::Char('s') if n_subjects > 0 => self.simulating = true,
            _ => {}
        }
    }

    fn sim_key(&mut self, key: KeyEvent) {
        let Some(name) = self.selected_subject().map(|s| s.name.clone()) else {
            self.simulating = false;
            return;
        };
        match key.code {
            KeyCode::Char(c @ '1'..='5') => {
                let w = self.sim_weight;
                self.sim.entry(name).or_default().push((c as u8 - b'0', w));
            }
            KeyCode::Char('w') => {
                self.sim_weight = match self.sim_weight as u32 {
                    100 => 200,
                    200 => 50,
                    _ => 100,
                } as f64
            }
            KeyCode::Backspace => {
                if let Some(v) = self.sim.get_mut(&name) {
                    v.pop();
                }
            }
            KeyCode::Char('c') => {
                self.sim.remove(&name);
            }
            KeyCode::Esc | KeyCode::Char('s') | KeyCode::Enter => self.simulating = false,
            KeyCode::Char('j') | KeyCode::Down => {
                self.subject_idx = (self.subject_idx + 1).min(self.data.subjects.len().saturating_sub(1))
            }
            KeyCode::Char('k') | KeyCode::Up => self.subject_idx = self.subject_idx.saturating_sub(1),
            _ => {}
        }
    }

    pub fn selected_subject(&self) -> Option<&SubjectStats> {
        self.data.subjects.get(self.subject_idx)
    }

    fn timetable_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => self.day_idx = self.day_idx.saturating_sub(1),
            KeyCode::Char('l') | KeyCode::Right => self.day_idx = (self.day_idx + 1).min(self.week_days() - 1),
            KeyCode::Char('j') | KeyCode::Down => self.lesson_row += 1,
            KeyCode::Char('k') | KeyCode::Up => self.lesson_row = self.lesson_row.saturating_sub(1),
            KeyCode::Char('n') | KeyCode::Char(']') | KeyCode::PageDown => self.change_week(7),
            KeyCode::Char('p') | KeyCode::Char('[') | KeyCode::PageUp => self.change_week(-7),
            KeyCode::Char('t') => {
                let today = Local::now().date_naive();
                self.week = monday_of(today);
                self.day_idx = (today.weekday().num_days_from_monday() as usize).min(4);
                self.ensure_week();
            }
            _ => {}
        }
    }

    fn change_week(&mut self, days: i64) {
        self.week = self.week.checked_add_signed(chrono::Duration::days(days)).unwrap_or(self.week);
        self.day_idx = self.day_idx.min(self.week_days() - 1);
        self.ensure_week();
    }

    fn ensure_week(&mut self) {
        if !self.raw.timetable.contains_key(&self.week.to_string()) {
            self.fetch_week(self.week);
        }
    }

    /// 5 on normal weeks, 6–7 if there are weekend lessons.
    pub fn week_days(&self) -> usize {
        let weekend = self
            .data
            .weeks
            .get(&self.week)
            .map(|ls| ls.iter().map(|l| l.start.weekday().num_days_from_monday() as usize + 1).max().unwrap_or(5))
            .unwrap_or(5);
        weekend.max(5)
    }

    fn absences_key(&mut self, key: KeyEvent) {
        let n = self.data.absences.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.absence_idx = (self.absence_idx + 1).min(n.saturating_sub(1)),
            KeyCode::Char('k') | KeyCode::Up => self.absence_idx = self.absence_idx.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => self.absence_idx = 0,
            KeyCode::Char('G') | KeyCode::End => self.absence_idx = n.saturating_sub(1),
            _ => {}
        }
    }

    fn tasks_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.tasks_scroll = self.tasks_scroll.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => self.tasks_scroll = self.tasks_scroll.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => self.tasks_scroll = 0,
            _ => {}
        }
    }
}
