//! Typed views over the raw KRÉTA JSON. Parsing is lenient on purpose:
//! unknown or missing fields fall back to empty values instead of failing.

use chrono::{DateTime, Local};
use serde_json::Value;

use super::client::JsonExt;

fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or_default()
}

#[derive(Debug, Clone, Default)]
pub struct Student {
    pub name: String,
    pub school: String,
}

impl Student {
    pub fn from_json(v: &Value) -> Self {
        Self { name: v.str_at("Nev"), school: v.str_at("IntezmenyNev") }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GradeKind {
    MidYear,
    HalfYear,
    EndYear,
    Other(String),
}

#[derive(Debug, Clone)]
pub struct Grade {
    pub subject: String,
    pub date: DateTime<Local>,
    pub recorded: DateTime<Local>,
    /// 1–5 for regular grades, 0 for textual/percentage evaluations.
    pub value: u8,
    pub text: String,
    pub weight: f64,
    pub topic: String,
    pub mode: String,
    pub teacher: String,
    pub kind: GradeKind,
    pub percentage: bool,
}

impl Grade {
    pub fn from_json(v: &Value) -> Self {
        let date = v.date_at("KeszitesDatuma").unwrap_or_default();
        let kind = match v.path_str(&["Tipus", "Nev"]).as_str() {
            "evkozi_jegy_ertekeles" => GradeKind::MidYear,
            "felevi_jegy_ertekeles" => GradeKind::HalfYear,
            "evvegi_jegy_ertekeles" => GradeKind::EndYear,
            other => GradeKind::Other(other.to_owned()),
        };
        Self {
            subject: v.path_str(&["Tantargy", "Nev"]),
            recorded: v.date_at("RogzitesDatuma").unwrap_or(date),
            date,
            value: v.f64_at("SzamErtek").unwrap_or(0.0).clamp(0.0, 5.0) as u8,
            text: v.str_at("SzovegesErtek"),
            weight: v.f64_at("SulySzazalekErteke").unwrap_or(100.0),
            topic: v.str_at("Tema"),
            mode: v.path_str(&["Mod", "Leiras"]),
            teacher: v.str_at("ErtekeloTanarNeve"),
            kind,
            percentage: v.path_str(&["ErtekFajta", "Uid"]) == "3,Szazalekos",
        }
    }

    /// Whether the grade counts toward the running average.
    pub fn counts(&self) -> bool {
        self.kind == GradeKind::MidYear && (1..=5).contains(&self.value) && !self.percentage && self.weight > 0.0
    }

    pub fn display_value(&self) -> String {
        match self.value {
            1..=5 => self.value.to_string(),
            _ if !self.text.is_empty() => self.text.clone(),
            _ => "–".into(),
        }
    }
}

pub fn grades(v: &Value) -> Vec<Grade> {
    let mut out: Vec<Grade> = list(v).iter().map(Grade::from_json).collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.date));
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Justification {
    Justified,
    Pending,
    Unjustified,
}

#[derive(Debug, Clone)]
pub struct Absence {
    pub subject: String,
    pub date: DateTime<Local>,
    pub lesson: Option<u32>,
    pub state: Justification,
    pub late_minutes: Option<u32>,
    pub justification_type: String,
}

pub fn absences(v: &Value) -> Vec<Absence> {
    let mut out: Vec<Absence> = list(v)
        .iter()
        .map(|a| {
            let delay = a.f64_at("KesesPercben").map(|m| m as u32);
            Absence {
                subject: a.path_str(&["Tantargy", "Nev"]),
                date: a.date_at("Datum").unwrap_or_default(),
                lesson: a.get("Ora").and_then(|o| o.f64_at("Oraszam")).map(|n| n as u32),
                state: match a.str_at("IgazolasAllapota").as_str() {
                    "Igazolt" => Justification::Justified,
                    "Igazolando" => Justification::Pending,
                    _ => Justification::Unjustified,
                },
                late_minutes: (a.path_str(&["Tipus", "Nev"]) == "keses").then_some(delay.unwrap_or(0)),
                justification_type: a.path_str(&["IgazolasTipusa", "Leiras"]),
            }
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse((a.date, a.lesson)));
    out
}

#[derive(Debug, Clone)]
pub struct Exam {
    pub subject: String,
    pub date: DateTime<Local>,
    pub lesson: Option<u32>,
    pub topic: String,
    pub mode: String,
    pub teacher: String,
}

pub fn exams(v: &Value) -> Vec<Exam> {
    let mut out: Vec<Exam> = list(v)
        .iter()
        .map(|e| {
            let subject = e.path_str(&["Tantargy", "Nev"]);
            Exam {
                subject: if subject.is_empty() { e.str_at("TantargyNeve") } else { subject },
                date: e.date_at("Datum").unwrap_or_default(),
                lesson: e.f64_at("OrarendiOraOraszama").map(|n| n as u32),
                topic: e.str_at("Temaja"),
                mode: e.path_str(&["Modja", "Leiras"]),
                teacher: e.str_at("RogzitoTanarNeve"),
            }
        })
        .collect();
    out.sort_by_key(|e| (e.date, e.lesson));
    out
}

#[derive(Debug, Clone)]
pub struct Homework {
    pub subject: String,
    pub deadline: DateTime<Local>,
    pub text: String,
    pub teacher: String,
}

pub fn homework(v: &Value) -> Vec<Homework> {
    let mut out: Vec<Homework> = list(v)
        .iter()
        .map(|h| Homework {
            subject: h.path_str(&["Tantargy", "Nev"]),
            deadline: h.date_at("HataridoDatuma").unwrap_or_default(),
            text: strip_html(&h.str_at("Szoveg")),
            teacher: h.str_at("RogzitoTanarNeve"),
        })
        .collect();
    out.sort_by_key(|h| h.deadline);
    out
}

#[derive(Debug, Clone)]
pub struct Note {
    pub title: String,
    pub date: DateTime<Local>,
    pub content: String,
    pub teacher: String,
}

pub fn notes(v: &Value) -> Vec<Note> {
    let mut out: Vec<Note> = list(v)
        .iter()
        .map(|n| Note {
            title: n.str_at("Cim"),
            date: n.date_at("Datum").unwrap_or_default(),
            content: strip_html(&n.str_at("Tartalom")),
            teacher: n.str_at("KeszitoTanarNeve"),
        })
        .collect();
    out.sort_by_key(|n| std::cmp::Reverse(n.date));
    out
}

#[derive(Debug, Clone)]
pub struct Lesson {
    pub subject: String,
    pub index: Option<u32>,
    pub start: DateTime<Local>,
    pub end: DateTime<Local>,
    pub room: String,
    pub teacher: String,
    pub substitute: Option<String>,
    pub topic: String,
    pub cancelled: bool,
    pub absent: bool,
    pub has_exam: bool,
    pub has_homework: bool,
}

pub fn lessons(v: &Value) -> Vec<Lesson> {
    let mut out: Vec<Lesson> = list(v)
        .iter()
        .map(|l| {
            let start = l.date_at("KezdetIdopont").unwrap_or_default();
            let subject = l.path_str(&["Tantargy", "Nev"]);
            let substitute = l.str_at("HelyettesTanarNeve");
            Lesson {
                subject: if subject.is_empty() { l.str_at("Nev") } else { subject },
                index: l.f64_at("Oraszam").map(|n| n as u32),
                end: l.date_at("VegIdopont").unwrap_or(start),
                start,
                room: l.str_at("TeremNeve").replace('_', " "),
                teacher: l.str_at("TanarNeve"),
                substitute: (!substitute.is_empty()).then_some(substitute),
                topic: l.str_at("Tema"),
                cancelled: l.path_str(&["Allapot", "Nev"]) == "Elmaradt",
                absent: l.path_str(&["TanuloJelenlet", "Nev"]) == "Hianyzas",
                has_exam: !l.str_at("BejelentettSzamonkeresUid").is_empty()
                    || l.get("BejelentettSzamonkeresUids").and_then(Value::as_array).is_some_and(|a| !a.is_empty()),
                has_homework: !l.str_at("HaziFeladatUid").is_empty(),
            }
        })
        .collect();
    out.sort_by_key(|l| l.start);
    out
}

fn strip_html(s: &str) -> String {
    let with_breaks = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n").replace("</p>", "\n");
    let mut out = String::with_capacity(with_breaks.len());
    let mut in_tag = false;
    for c in with_breaks.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&quot;", "\"").trim().to_owned()
}
