mod api;
mod app;
mod demo;
mod stats;
mod store;
mod ui;

use std::time::Duration;

use ratatui::crossterm::event::{self, Event as CtEvent, KeyEventKind};
use tokio::sync::mpsc;

use app::{App, Event};

const USAGE: &str = "kreta – terminálos kliens az e-KRÉTÁhoz

Használat: kreta [--demo | --logout | --debug-login]

  --demo          kitalált adatokkal indul, bejelentkezés nélkül
  --logout        törli a mentett munkamenetet és gyorsítótárat
  --debug-login   jelszavas belépés lépésenkénti naplóval (jelszó és tokenek nélkül)";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut demo = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--demo" => demo = true,
            "--debug-login" => return debug_login().await,
            "--logout" => {
                store::clear();
                println!("Kijelentkezve.");
                return Ok(());
            }
            _ => {
                println!("{USAGE}");
                return Ok(());
            }
        }
    }

    let (tx, mut rx) = mpsc::unbounded_channel();

    // Terminal input runs on its own thread; everything funnels into one channel.
    let input_tx = tx.clone();
    std::thread::spawn(move || {
        while let Ok(ev) = event::read() {
            let msg = match ev {
                CtEvent::Key(k) if k.kind != KeyEventKind::Release => Event::Key(k),
                CtEvent::Resize(..) => Event::Resize,
                _ => continue,
            };
            if input_tx.send(msg).is_err() {
                break;
            }
        }
    });
    let tick_tx = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(120));
        loop {
            interval.tick().await;
            if tick_tx.send(Event::Tick).is_err() {
                break;
            }
        }
    });

    let mut terminal = ratatui::init();
    let mut app = App::new(tx, demo);
    let result = async {
        let mut last_minute = None;
        loop {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            // Skip redraws on idle ticks unless something animates or the clock changes.
            loop {
                match rx.recv().await {
                    Some(Event::Key(k)) => app.on_key(k),
                    Some(Event::Msg(m)) => app.on_msg(m),
                    Some(Event::Resize) => {}
                    Some(Event::Tick) => {
                        app.tick += 1;
                        app.on_tick();
                        let minute = chrono::Local::now().format("%H:%M").to_string();
                        let busy = app.loading()
                            || matches!(&app.screen, app::Screen::Login(f) if f.busy || f.browser.is_some());
                        let status_expiring = app.status.as_ref().is_some_and(|s| s.2.elapsed().as_secs() <= 7);
                        if !busy && !status_expiring && last_minute.as_ref() == Some(&minute) {
                            continue;
                        }
                        last_minute = Some(minute);
                    }
                    None => return Ok(()),
                }
                break;
            }
            if app.quit {
                return Ok::<_, anyhow::Error>(());
            }
        }
    }
    .await;
    ratatui::restore();
    result
}

fn prompt(label: &str) -> anyhow::Result<String> {
    use std::io::Write;
    print!("{label}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_owned())
}

/// Read a line from the terminal without echoing it.
fn prompt_hidden(label: &str) -> anyhow::Result<String> {
    let stty = |arg: &str| {
        let tty = std::fs::File::open("/dev/tty").ok()?;
        std::process::Command::new("stty").arg(arg).stdin(tty).status().ok()
    };
    stty("-echo");
    let res = prompt(label);
    stty("echo");
    println!();
    res
}

/// Password login outside the TUI, printing every step for troubleshooting.
async fn debug_login() -> anyhow::Result<()> {
    println!("Jelszavas belépés teszt – a jelszó és a tokenek nem jelennek meg a kimenetben.\n");
    let query = prompt("Iskola (OM azonosító vagy név): ")?;
    let schools = api::auth::search_schools(&query).await?;
    let school = match schools.len() {
        0 => anyhow::bail!("nincs találat erre: {query}"),
        1 => schools[0].clone(),
        n => {
            for (i, s) in schools.iter().enumerate().take(20) {
                println!("  {:>2}. {} ({}, OM {})", i + 1, s.name, s.code, s.om);
            }
            let pick: usize = prompt(&format!("Melyik? [1-{}]: ", n.min(20)))?.parse()?;
            schools.get(pick.wrapping_sub(1)).cloned().ok_or_else(|| anyhow::anyhow!("érvénytelen szám"))?
        }
    };
    println!("Iskola: {} ({})", school.name, school.code);
    let user = prompt("Felhasználónév: ")?;
    let pass = prompt_hidden("Jelszó: ")?;
    println!();
    match api::auth::login_traced(&school.code, &user, &pass, &mut |l| println!("{l}")).await {
        Ok(_) => println!("\nSIKERES belépés. (A munkamenet nincs elmentve.)"),
        Err(e) => println!("\nHIBA: {e:#}"),
    }
    Ok(())
}
