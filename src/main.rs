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

Használat: kreta [--demo | --logout]

  --demo     kitalált adatokkal indul, bejelentkezés nélkül
  --logout   törli a mentett munkamenetet és gyorsítótárat";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut demo = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--demo" => demo = true,
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
