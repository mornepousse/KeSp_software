use crate::msg::{AppShared, BgMsg};
use crate::logic::{binary_protocol, protocol};
use crate::{MainWindow, AppState, StatsBridge};
use slint::{ComponentHandle, Model, SharedString};

pub fn setup(window: &MainWindow, shared: &AppShared) {
    setup_refresh_stats(window, shared);
    setup_export_csv(window, shared);
}

fn setup_refresh_stats(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<StatsBridge>().on_refresh_stats(move || {
        if let Some(w) = window_weak.upgrade() {
            w.global::<AppState>().set_status_text("Loading key statistics...".into());
        }
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut ser = match serial.lock() {
                Ok(s) => s,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("Lock error: {}", e)));
                    return;
                }
            };
            if ser.v2 {
                match ser.send_binary(binary_protocol::cmd::KEYSTATS_BIN, &[]) {
                    Ok(resp) => {
                        let (data, max_val) = crate::logic::parsers::parse_keystats_binary(&resp.payload);
                        tx.send(BgMsg::StatsData(data, max_val));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Stats error: {}", e))); }
                }
            } else {
                match ser.query_command(protocol::CMD_KEYSTATS) {
                    Ok(lines) => {
                        let (data, max_val) = crate::logic::parsers::parse_heatmap_lines(&lines);
                        tx.send(BgMsg::StatsData(data, max_val));
                    }
                    Err(e) => { tx.send(BgMsg::StatusMsg(format!("Stats error: {}", e))); }
                }
            }
        });
    });
}

fn setup_export_csv(window: &MainWindow, shared: &AppShared) {
    let window_weak = window.as_weak();
    let keys_arc = shared.keys.clone();
    let current_keymap = shared.current_keymap.clone();
    window.global::<StatsBridge>().on_export_csv(move || {
        let window_weak = window_weak.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();

        // Read the current heatmap keycaps from the bridge to get labels + colors
        let heatmap_keycaps = if let Some(w) = window_weak.upgrade() {
            let bridge = w.global::<StatsBridge>();
            let model = bridge.get_heatmap_keycaps();
            let count = model.row_count();
            let mut items = Vec::with_capacity(count);
            for i in 0..count {
                items.push(model.row_data(i).unwrap());
            }
            items
        } else {
            return;
        };

        if heatmap_keycaps.is_empty() {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("No stats data. Click Refresh Stats first.".into());
            }
            return;
        }

        // Pick save path
        let dialog = rfd::FileDialog::new()
            .set_title("Export Key Statistics CSV")
            .add_filter("CSV", &["csv"])
            .set_file_name("keystats.csv");

        if let Some(path) = dialog.save_file() {
            let km = current_keymap.borrow();
            let mut csv = String::from("Row,Col,Label,Keycode,Presses\n");
            let keys_guard = keys_arc.lock().unwrap();
            for (i, kp) in keys_guard.iter().enumerate() {
                let row = kp.row as usize;
                let col = kp.col as usize;
                let label = if i < heatmap_keycaps.len() {
                    heatmap_keycaps[i].label.to_string()
                } else {
                    String::new()
                };
                let code = km.get(row).and_then(|r| r.get(col)).copied().unwrap_or(0);
                let sublabel = if i < heatmap_keycaps.len() {
                    heatmap_keycaps[i].sublabel.to_string()
                } else {
                    "0".to_string()
                };
                // sublabel stores the press count as a string in heatmap mode
                csv.push_str(&format!("{},{},{},0x{:04X},{}\n", row, col, label, code, sublabel));
            }
            match std::fs::write(&path, &csv) {
                Ok(_) => {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<AppState>().set_status_text(
                            SharedString::from(format!("Exported to {}", path.display()))
                        );
                    }
                }
                Err(e) => {
                    if let Some(w) = window_weak.upgrade() {
                        w.global::<AppState>().set_status_text(
                            SharedString::from(format!("Export error: {}", e))
                        );
                    }
                }
            }
        }
    });
}
