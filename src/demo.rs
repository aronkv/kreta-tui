//! Made-up data in the KRÉTA JSON shape, for `--demo` and UI tests.

use chrono::{Datelike, Days, Local, NaiveDate, TimeZone};
use serde_json::{Value, json};

use crate::app::monday_of;
use crate::store::RawData;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.next() as usize % xs.len()]
    }
}

const SUBJECTS: [(&str, &str, f64); 9] = [
    ("Matematika", "Kovács Éva", 3.6),
    ("Magyar irodalom", "Szabó Péter", 4.3),
    ("Magyar nyelv", "Szabó Péter", 4.0),
    ("Történelem", "Nagy Gábor", 4.6),
    ("Angol nyelv", "Tóth Anna", 4.8),
    ("Fizika", "Horváth Zoltán", 3.1),
    ("Kémia", "Varga Judit", 3.4),
    ("Biológia", "Kiss Márta", 4.2),
    ("Informatika", "Molnár Ádám", 4.9),
];

const GRADE_NAMES: [&str; 6] = ["", "Elégtelen", "Elégséges", "Közepes", "Jó", "Jeles"];

const TIMES: [(u32, u32); 8] = [(8, 0), (8, 55), (9, 50), (10, 50), (11, 45), (12, 40), (13, 35), (14, 25)];

fn iso(d: NaiveDate, h: u32, m: u32) -> String {
    Local
        .from_local_datetime(&d.and_hms_opt(h, m, 0).unwrap())
        .unwrap()
        .with_timezone(&chrono::Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn subject(name: &str) -> Value {
    json!({ "Uid": name, "Nev": name, "Kategoria": { "Nev": name } })
}

fn week_lessons(monday: NaiveDate, rng: &mut Lcg) -> Value {
    let mut out = Vec::new();
    for day in 0..5u64 {
        let date = monday + Days::new(day);
        let count = 5 + (day as usize + monday.iso_week().week() as usize) % 3;
        for (i, (h, m)) in TIMES.iter().take(count).enumerate() {
            let (name, teacher, _) = SUBJECTS[(day as usize * 3 + i * 2) % SUBJECTS.len()];
            let roll = rng.next() % 40;
            out.push(json!({
                "Uid": format!("{date}-{i}"),
                "Tantargy": subject(name),
                "Oraszam": i + 1,
                "KezdetIdopont": iso(date, *h, *m),
                "VegIdopont": iso(date, h + (m + 45) / 60, (m + 45) % 60),
                "TeremNeve": format!("{}{:02}", 1 + i % 3, 2 + (day as usize * 7 + i) % 14),
                "TanarNeve": teacher,
                "HelyettesTanarNeve": if roll == 0 { Value::from("Fekete Béla") } else { Value::Null },
                "Allapot": { "Nev": if roll == 1 { "Elmaradt" } else { "Naplozott" } },
                "Tema": if roll.is_multiple_of(3) { "Ismétlés" } else { "" },
                "BejelentettSzamonkeresUid": if roll == 2 { Value::from("x") } else { Value::Null },
                "HaziFeladatUid": if roll == 3 { Value::from("h") } else { Value::Null },
            }));
        }
    }
    Value::Array(out)
}

pub fn data() -> RawData {
    let mut rng = Lcg(42);
    let today = Local::now().date_naive();
    let start = today - Days::new(70);

    let modes =
        ["Írásbeli témazáró dolgozat", "Szóbeli felelet", "Írásbeli röpdolgozat", "Házi feladat", "Projektmunka"];
    let topics = [
        "Másodfokú egyenletek",
        "Petőfi lírája",
        "Az Árpád-ház",
        "Present Perfect",
        "Newton törvényei",
        "Sejtosztódás",
        "Algoritmusok",
        "",
    ];
    let mut grades = Vec::new();
    for (si, (name, teacher, skill)) in SUBJECTS.iter().enumerate() {
        let n = 5 + si % 4;
        for k in 0..n {
            let mut day = start + Days::new((k as u64 * 70 / n as u64) + rng.next() % 6);
            while day.weekday().num_days_from_monday() > 4 {
                day = day - Days::new(1);
            }
            let noise = (rng.next() % 100) as f64 / 100.0 * 2.2 - 1.1;
            let value = (skill + noise).round().clamp(1.0, 5.0) as u8;
            let mode = *rng.pick(&modes);
            let weight = if mode.contains("témazáró") { 200 } else { 100 };
            grades.push(json!({
                "Uid": format!("{si}-{k}"),
                "Tantargy": subject(name),
                "KeszitesDatuma": iso(day, 9, 0),
                "RogzitesDatuma": iso(day, 15, 0),
                "SzamErtek": value,
                "SzovegesErtek": GRADE_NAMES[value as usize],
                "SulySzazalekErteke": weight,
                "Tema": rng.pick(&topics),
                "Mod": { "Leiras": mode },
                "ErtekeloTanarNeve": teacher,
                "Tipus": { "Nev": "evkozi_jegy_ertekeles" },
                "ErtekFajta": { "Uid": "1,Osztalyzat" },
            }));
        }
    }

    let mut absences = Vec::new();
    for k in 0..14u64 {
        let day = start + Days::new(k * 5 + rng.next() % 3);
        if day.weekday().num_days_from_monday() > 4 {
            continue;
        }
        let (name, teacher, _) = *rng.pick(&SUBJECTS);
        let late = k % 5 == 0;
        absences.push(json!({
            "Tantargy": subject(name),
            "Datum": iso(day, 0, 0),
            "Ora": { "Oraszam": 1 + rng.next() % 6 },
            "IgazolasAllapota": *rng.pick(&["Igazolt", "Igazolt", "Igazolt", "Igazolando", "Igazolatlan"]),
            "IgazolasTipusa": { "Leiras": "Szülői igazolás" },
            "Tipus": { "Nev": if late { "keses" } else { "hianyzas" } },
            "KesesPercben": if late { Value::from(3 + rng.next() % 12) } else { Value::Null },
            "RogzitoTanarNeve": teacher,
        }));
    }

    let exams: Vec<Value> = [
        (2u64, "Matematika", "Függvények"),
        (5, "Fizika", "Mozgástan"),
        (9, "Történelem", "Középkor"),
        (13, "Kémia", "Redoxi reakciók"),
    ]
    .iter()
    .map(|(d, s, t)| {
        json!({
            "Datum": iso(today + Days::new(*d), 0, 0),
            "Tantargy": subject(s),
            "Temaja": t,
            "Modja": { "Leiras": "Írásbeli témazáró dolgozat" },
            "OrarendiOraOraszama": 2,
            "RogzitoTanarNeve": "Kovács Éva",
        })
    })
    .collect();

    let homework = json!([
        { "Tantargy": subject("Angol nyelv"), "HataridoDatuma": iso(today + Days::new(1), 0, 0), "Szoveg": "Workbook p. 42, 3–5. feladat", "RogzitoTanarNeve": "Tóth Anna" },
        { "Tantargy": subject("Matematika"), "HataridoDatuma": iso(today + Days::new(3), 0, 0), "Szoveg": "Tankönyv 118. oldal<br>1., 2/b, 4.", "RogzitoTanarNeve": "Kovács Éva" },
    ]);
    let notes = json!([
        { "Cim": "Szülői értekezlet", "Datum": iso(today - Days::new(4), 0, 0), "Tartalom": "Október 15-én 17:00-kor szülői értekezlet a 204-es teremben.", "KeszitoTanarNeve": "Kiss Márta" },
    ]);

    let this_week = monday_of(today);
    let mut timetable = std::collections::BTreeMap::new();
    for w in [this_week, this_week + Days::new(7), this_week - Days::new(7)] {
        timetable.insert(w.to_string(), week_lessons(w, &mut rng));
    }

    RawData {
        student: json!({ "Nev": "Minta Diák", "IntezmenyNev": "Demó Gimnázium" }),
        grades: Value::Array(grades),
        absences: Value::Array(absences),
        exams: Value::Array(exams),
        homework,
        notes,
        timetable,
        fetched_at: Some(Local::now()),
    }
}
