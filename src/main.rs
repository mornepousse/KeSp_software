mod logic;
mod msg;
mod models;
mod keycode_catalog;
mod bridge;

slint::include_modules!();

use logic::keycode;
use logic::layout::KeycapPos;
use logic::serial::SerialManager;
use models::{build_keycap_model, build_layer_model, build_macro_list, update_keycap_labels, heatmap_color};
use msg::{AppShared, UiSender, BgMsg};
use slint::{Model, ModelRc, SharedString, VecModel};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::rc::Rc;

fn main() {
    let keys = logic::layout::default_layout();
    let keys_arc: Arc<Mutex<Vec<KeycapPos>>> = Arc::new(Mutex::new(keys.clone()));

    let keycap_model = build_keycap_model(&keys);
    let layer_model = build_layer_model(&["Layer 0".into(), "Layer 1".into(), "Layer 2".into(), "Layer 3".into()]);

    let window = MainWindow::new().unwrap();

    // Set up models
    let keymap_bridge = window.global::<KeymapBridge>();
    keymap_bridge.set_keycaps(ModelRc::from(keycap_model.clone()));
    keymap_bridge.set_layers(ModelRc::from(layer_model.clone()));

    // Set layout bounding box for responsive keyboard scaling
    {
        let keys_guard = keys_arc.lock().unwrap();
        let (bw, bh) = logic::layout::bounding_box(&keys_guard);
        keymap_bridge.set_layout_width(bw);
        keymap_bridge.set_layout_height(bh);
        window.global::<StatsBridge>().set_layout_width(bw);
        window.global::<StatsBridge>().set_layout_height(bh);
    }

    // Serial manager shared between threads
    let serial: Arc<Mutex<SerialManager>> = Arc::new(Mutex::new(SerialManager::new()));
    let (raw_tx, bg_rx) = mpsc::channel::<BgMsg>();
    let wake_fn: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {
        let _ = slint::invoke_from_event_loop(|| {});
    });
    let bg_tx = UiSender::new(raw_tx, wake_fn);

    // Current state
    let current_keymap: Rc<std::cell::RefCell<Vec<Vec<u16>>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let current_layer: Rc<std::cell::Cell<usize>> = Rc::new(std::cell::Cell::new(0));
    let saved_settings = logic::settings::load();
    let keyboard_layout: Rc<std::cell::RefCell<logic::layout_remap::KeyboardLayout>> =
        Rc::new(std::cell::RefCell::new(logic::layout_remap::KeyboardLayout::from_name(&saved_settings.keyboard_layout)));

    // Macro editor state
    let macro_steps: Rc<std::cell::RefCell<Vec<(String, u8, u32)>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    let macro_entries: Rc<std::cell::RefCell<Vec<logic::parsers::MacroEntry>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));

    // OTA / Flash file paths
    let ota_firmware_path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let flash_firmware_path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    // Build shared state struct for bridge modules
    let shared = AppShared {
        serial: serial.clone(),
        tx: bg_tx.clone(),
        keys: keys_arc.clone(),
        current_keymap: current_keymap.clone(),
        keyboard_layout: keyboard_layout.clone(),
        keycap_model: keycap_model.clone(),
        layer_model: layer_model.clone(),
        current_layer: current_layer.clone(),
        macro_steps: macro_steps.clone(),
        macro_entries: macro_entries.clone(),
        ota_firmware_path,
        flash_firmware_path,
    };

    // Wire up all bridge callbacks
    bridge::setup_all(&window, &shared);

    // --- Process background messages + auto-reconnect + WPM polling ---
    {
        let window_weak = window.as_weak();
        let keycap_model = keycap_model.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let keyboard_layout = keyboard_layout.clone();
        let macro_entries = macro_entries.clone();
        let serial_timer = serial.clone();
        let tx_timer = bg_tx.clone();

        let tick_counter = std::cell::Cell::new(0u32);
        let reconnect_in_progress = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let notification_countdown = std::cell::Cell::new(0u32);

        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(200),
            move || {
                let Some(window) = window_weak.upgrade() else { return };

                let ticks = tick_counter.get().wrapping_add(1);
                tick_counter.set(ticks);

                // Notification auto-hide logic
                {
                    let app = window.global::<AppState>();
                    if app.get_notification_visible() {
                        let count = notification_countdown.get();
                        if count == 0 {
                            notification_countdown.set(15);
                        } else if count == 1 {
                            app.set_notification_visible(false);
                            notification_countdown.set(0);
                        } else {
                            notification_countdown.set(count - 1);
                        }
                    } else {
                        notification_countdown.set(0);
                    }
                }

                // Auto-reconnect every 15 ticks (3 seconds)
                if ticks % 15 == 0 {
                    let is_disconnected = window.global::<AppState>().get_connection() == ConnectionState::Disconnected;
                    let not_already_trying = !reconnect_in_progress.load(std::sync::atomic::Ordering::Relaxed);
                    if is_disconnected && not_already_trying {
                        let serial = serial_timer.clone();
                        let tx = tx_timer.clone();
                        let flag = reconnect_in_progress.clone();
                        flag.store(true, std::sync::atomic::Ordering::Relaxed);
                        std::thread::spawn(move || {
                            let mut ser = match serial.lock() {
                                Ok(s) => s,
                                Err(_) => {
                                    flag.store(false, std::sync::atomic::Ordering::Relaxed);
                                    return;
                                }
                            };
                            match ser.auto_connect() {
                                Ok(port_name) => {
                                    let fw = ser.get_firmware_version().unwrap_or_default();
                                    let names = ser.get_layer_names().unwrap_or_default();
                                    let km = ser.get_keymap(0).unwrap_or_default();
                                    tx.send(BgMsg::Connected(port_name, fw, names, km));

                                    if let Ok(json) = ser.get_layout_json() {
                                        if let Ok(keys) = logic::layout::parse_json(&json) {
                                            tx.send(BgMsg::LayoutJson(keys));
                                        }
                                    }
                                }
                                Err(_) => {}
                            }
                            flag.store(false, std::sync::atomic::Ordering::Relaxed);
                        });
                    }
                }

                // WPM polling every 10 ticks (2 seconds)
                if ticks % 10 == 0 {
                    let is_connected = window.global::<AppState>().get_connection() == ConnectionState::Connected;
                    if is_connected {
                        let serial = serial_timer.clone();
                        let tx = tx_timer.clone();
                        std::thread::spawn(move || {
                            if let Ok(mut ser) = serial.lock() {
                                if ser.connected {
                                    if let Ok(wpm) = ser.get_wpm() {
                                        tx.send(BgMsg::Wpm(wpm));
                                    }
                                }
                            }
                        });
                    }
                }

                while let Ok(msg) = bg_rx.try_recv() {
                    match msg {
                        BgMsg::Connected(port, fw, names, km) => {
                            let app = window.global::<AppState>();
                            app.set_connection(ConnectionState::Connected);
                            app.set_firmware_version(SharedString::from(&fw));
                            app.set_status_text(SharedString::from(format!("Connected to {}", port)));

                            let new_layers = build_layer_model(&names);
                            window.global::<KeymapBridge>().set_layers(ModelRc::from(new_layers));

                            *current_keymap.borrow_mut() = km.clone();
                            update_keycap_labels(&keycap_model, &keys_arc.lock().unwrap(), &km, &keyboard_layout.borrow());
                        }
                        BgMsg::ConnectError(e) => {
                            let app = window.global::<AppState>();
                            app.set_connection(ConnectionState::Disconnected);
                            app.set_status_text(SharedString::from(format!("Error: {}", e)));
                        }
                        BgMsg::Keymap(km) => {
                            *current_keymap.borrow_mut() = km.clone();
                            update_keycap_labels(&keycap_model, &keys_arc.lock().unwrap(), &km, &keyboard_layout.borrow());
                            window.global::<AppState>().set_status_text("Keymap loaded".into());
                        }
                        BgMsg::LayerNames(names) => {
                            let new_layers = build_layer_model(&names);
                            window.global::<KeymapBridge>().set_layers(ModelRc::from(new_layers));
                        }
                        BgMsg::LayoutJson(new_keys) => {
                            *keys_arc.lock().unwrap() = new_keys.clone();

                            let count = keycap_model.row_count();
                            for _ in 0..count {
                                keycap_model.remove(0);
                            }
                            for (idx, kp) in new_keys.iter().enumerate() {
                                keycap_model.push(KeycapData {
                                    x: kp.x,
                                    y: kp.y,
                                    w: kp.w,
                                    h: kp.h,
                                    rotation: kp.angle,
                                    rotation_cx: kp.w / 2.0,
                                    rotation_cy: kp.h / 2.0,
                                    label: SharedString::from(format!("{},{}", kp.col, kp.row)),
                                    sublabel: SharedString::default(),
                                    keycode: 0,
                                    color: slint::Color::from_argb_u8(255, 0x44, 0x47, 0x5a),
                                    selected: false,
                                    index: idx as i32,
                                });
                            }

                            let km = current_keymap.borrow();
                            if !km.is_empty() {
                                update_keycap_labels(&keycap_model, &new_keys, &km, &keyboard_layout.borrow());
                            }

                            let (bw, bh) = logic::layout::bounding_box(&new_keys);
                            window.global::<KeymapBridge>().set_layout_width(bw);
                            window.global::<KeymapBridge>().set_layout_height(bh);
                            window.global::<StatsBridge>().set_layout_width(bw);
                            window.global::<StatsBridge>().set_layout_height(bh);

                            window.global::<AppState>().set_status_text(
                                SharedString::from(format!("Layout received from firmware ({} keys)", new_keys.len()))
                            );
                        }
                        BgMsg::Disconnected => {
                            let app = window.global::<AppState>();
                            app.set_connection(ConnectionState::Disconnected);
                            app.set_firmware_version(SharedString::default());
                            app.set_status_text("Disconnected".into());
                        }
                        BgMsg::TapDanceData(td_slots) => {
                            let model: Vec<TapDanceSlot> = td_slots.iter().enumerate().map(|(i, actions)| {
                                TapDanceSlot {
                                    index: i as i32,
                                    tap1: SharedString::from(logic::keycode::decode_keycode(actions[0])),
                                    tap2: SharedString::from(logic::keycode::decode_keycode(actions[1])),
                                    tap3: SharedString::from(logic::keycode::decode_keycode(actions[2])),
                                    hold: SharedString::from(logic::keycode::decode_keycode(actions[3])),
                                    tap1_code: actions[0] as i32,
                                    tap2_code: actions[1] as i32,
                                    tap3_code: actions[2] as i32,
                                    hold_code: actions[3] as i32,
                                }
                            }).collect();
                            let vec_model = Rc::new(VecModel::from(model));
                            window.global::<AdvancedBridge>().set_tap_dance_slots(ModelRc::from(vec_model));
                        }
                        BgMsg::ComboData(combos) => {
                            let km = current_keymap.borrow();
                            let layout = keyboard_layout.borrow();
                            let model: Vec<ComboEntry> = combos.iter().map(|c| {
                                let key1_label = km.get(c.r1 as usize)
                                    .and_then(|row| row.get(c.c1 as usize))
                                    .map(|&code| {
                                        let decoded = keycode::decode_keycode(code);
                                        let remapped = logic::layout_remap::remap_key_label(&layout, &decoded);
                                        remapped.unwrap_or(&decoded).to_string()
                                    })
                                    .unwrap_or_else(|| format!("r{}c{}", c.r1, c.c1));
                                let key2_label = km.get(c.r2 as usize)
                                    .and_then(|row| row.get(c.c2 as usize))
                                    .map(|&code| {
                                        let decoded = keycode::decode_keycode(code);
                                        let remapped = logic::layout_remap::remap_key_label(&layout, &decoded);
                                        remapped.unwrap_or(&decoded).to_string()
                                    })
                                    .unwrap_or_else(|| format!("r{}c{}", c.r2, c.c2));
                                ComboEntry {
                                    index: c.index as i32,
                                    key1_label: SharedString::from(key1_label),
                                    key2_label: SharedString::from(key2_label),
                                    result_label: SharedString::from(logic::keycode::decode_keycode(c.result)),
                                    key1_row: c.r1 as i32,
                                    key1_col: c.c1 as i32,
                                    key2_row: c.r2 as i32,
                                    key2_col: c.c2 as i32,
                                    result_code: c.result as i32,
                                }
                            }).collect();
                            let vec_model = Rc::new(VecModel::from(model));
                            window.global::<AdvancedBridge>().set_combos(ModelRc::from(vec_model));
                        }
                        BgMsg::LeaderData(leaders) => {
                            let model: Vec<LeaderEntry> = leaders.iter().map(|l| {
                                let seq_str = l.sequence.iter()
                                    .map(|&k| keycode::hid_key_name(k))
                                    .collect::<Vec<_>>()
                                    .join(" -> ");
                                let key_name = keycode::hid_key_name(l.result);
                                let result_str = if l.result_mod != 0 {
                                    format!("{} + {}", key_name, keycode::mod_name(l.result_mod))
                                } else {
                                    key_name
                                };
                                LeaderEntry {
                                    index: l.index as i32,
                                    sequence: SharedString::from(seq_str),
                                    result: SharedString::from(result_str),
                                    result_code: l.result as i32,
                                    result_mod: l.result_mod as i32,
                                }
                            }).collect();
                            let vec_model = Rc::new(VecModel::from(model));
                            window.global::<AdvancedBridge>().set_leaders(ModelRc::from(vec_model));
                        }
                        BgMsg::KoData(kos) => {
                            let model: Vec<KeyOverrideEntry> = kos.iter().enumerate().map(|(i, ko)| {
                                let trig_key = keycode::hid_key_name(ko[0]);
                                let trig_mod = if ko[1] != 0 { keycode::mod_name(ko[1]) } else { "None".to_string() };
                                let res_key = keycode::hid_key_name(ko[2]);
                                let res_mod = if ko[3] != 0 { keycode::mod_name(ko[3]) } else { "None".to_string() };
                                KeyOverrideEntry {
                                    index: i as i32,
                                    trigger_key: SharedString::from(trig_key),
                                    trigger_mod: SharedString::from(trig_mod),
                                    result_key: SharedString::from(res_key),
                                    result_mod: SharedString::from(res_mod),
                                }
                            }).collect();
                            let vec_model = Rc::new(VecModel::from(model));
                            window.global::<AdvancedBridge>().set_key_overrides(ModelRc::from(vec_model));
                        }
                        BgMsg::BtData(bt_lines) => {
                            let mut active_slot: i32 = 0;
                            let mut connected: u8 = 0;
                            let mut bt_mode = "USB";
                            let mut slots: Vec<BtSlotInfo> = Vec::new();

                            for line in &bt_lines {
                                if line.starts_with("BT:") {
                                    if let Some(s) = line.find("slot=") {
                                        let rest = &line[s + 5..];
                                        if let Some(val) = rest.split_whitespace().next() {
                                            active_slot = val.parse().unwrap_or(0);
                                        }
                                    }
                                    if let Some(s) = line.find("conn=") {
                                        let rest = &line[s + 5..];
                                        if let Some(val) = rest.split_whitespace().next() {
                                            connected = val.parse().unwrap_or(0);
                                        }
                                    }
                                    if let Some(s) = line.find("init=") {
                                        let rest = &line[s + 5..];
                                        if let Some(val) = rest.split_whitespace().next() {
                                            let init: u8 = val.parse().unwrap_or(0);
                                            bt_mode = if init != 0 { "BT" } else { "USB" };
                                        }
                                    }
                                } else if line.starts_with("BT slot") {
                                    let slot_idx = line.chars()
                                        .skip(8)
                                        .take_while(|c| c.is_ascii_digit())
                                        .collect::<String>()
                                        .parse::<i32>()
                                        .unwrap_or(0);

                                    let valid = line.find("valid=")
                                        .and_then(|s| line[s + 6..].split_whitespace().next())
                                        .and_then(|v| v.parse::<u8>().ok())
                                        .unwrap_or(0);

                                    let addr = line.find("addr=")
                                        .and_then(|s| line[s + 5..].split_whitespace().next())
                                        .unwrap_or("")
                                        .to_string();

                                    let name = line.find("name=")
                                        .map(|s| line[s + 5..].trim().to_string())
                                        .unwrap_or_default();

                                    let status = if connected != 0 && slot_idx == active_slot {
                                        "Connected"
                                    } else if valid != 0 {
                                        "Paired"
                                    } else {
                                        "Empty"
                                    };

                                    slots.push(BtSlotInfo {
                                        index: slot_idx,
                                        status: SharedString::from(status),
                                        name: SharedString::from(name),
                                        addr: SharedString::from(addr),
                                    });
                                }
                            }

                            let bridge = window.global::<AdvancedBridge>();
                            bridge.set_bt_active_slot(active_slot);
                            bridge.set_bt_mode(SharedString::from(bt_mode));
                            let vec_model = Rc::new(VecModel::from(slots));
                            bridge.set_bt_slots(ModelRc::from(vec_model));
                        }
                        BgMsg::StatsData(heatmap_data, max_val) => {
                            let mut total: u32 = 0;
                            for row in &heatmap_data {
                                for &count in row {
                                    total += count;
                                }
                            }

                            let km = current_keymap.borrow();
                            let keys_guard = keys_arc.lock().unwrap();
                            let heatmap_keycaps: Vec<KeycapData> = keys_guard
                                .iter()
                                .enumerate()
                                .map(|(idx, kp)| {
                                    let row = kp.row as usize;
                                    let col = kp.col as usize;
                                    let count = heatmap_data
                                        .get(row)
                                        .and_then(|r| r.get(col))
                                        .copied()
                                        .unwrap_or(0);
                                    let value = if max_val > 0 {
                                        count as f32 / max_val as f32
                                    } else {
                                        0.0
                                    };
                                    let color = heatmap_color(value);

                                    let code = km.get(row).and_then(|r| r.get(col)).copied().unwrap_or(0);
                                    let decoded = keycode::decode_keycode(code);
                                    let layout = keyboard_layout.borrow();
                                    let remapped = logic::layout_remap::remap_key_label(&layout, &decoded);
                                    let label = remapped.unwrap_or(&decoded).to_string();

                                    KeycapData {
                                        x: kp.x,
                                        y: kp.y,
                                        w: kp.w,
                                        h: kp.h,
                                        rotation: kp.angle,
                                        rotation_cx: kp.w / 2.0,
                                        rotation_cy: kp.h / 2.0,
                                        label: SharedString::from(label),
                                        sublabel: SharedString::from(format!("{}", count)),
                                        keycode: code as i32,
                                        color,
                                        selected: false,
                                        index: idx as i32,
                                    }
                                })
                                .collect();

                            let heatmap_model = Rc::new(VecModel::from(heatmap_keycaps));
                            let stats_bridge = window.global::<StatsBridge>();
                            stats_bridge.set_heatmap_keycaps(ModelRc::from(heatmap_model));
                            stats_bridge.set_total_keypresses(total as i32);

                            let balance = logic::stats_analyzer::hand_balance(&heatmap_data);
                            let fingers = logic::stats_analyzer::finger_load(&heatmap_data);
                            let rows = logic::stats_analyzer::row_usage(&heatmap_data);
                            let top = logic::stats_analyzer::top_keys(&heatmap_data, &km, 10);
                            let dead = logic::stats_analyzer::dead_keys(&heatmap_data, &km);

                            let mut summary = String::new();
                            summary.push_str(&format!(
                                "Hand Balance:  Left {:.1}% ({})  |  Right {:.1}% ({})\n\n",
                                balance.left_pct, balance.left_count,
                                balance.right_pct, balance.right_count
                            ));

                            summary.push_str("Finger Load:\n");
                            for f in &fingers {
                                if f.count > 0 {
                                    summary.push_str(&format!("  {:>10}  {:5.1}%  ({})\n", f.name, f.pct, f.count));
                                }
                            }

                            summary.push_str("\nRow Usage:\n");
                            for r in &rows {
                                summary.push_str(&format!("  {:>8}  {:5.1}%  ({})\n", r.name, r.pct, r.count));
                            }

                            if !top.is_empty() {
                                summary.push_str("\nTop Keys:\n");
                                for (i, k) in top.iter().enumerate() {
                                    summary.push_str(&format!(
                                        "  {:2}. {:>8} ({:>8})  {:5.1}%  ({})\n",
                                        i + 1, k.name, k.finger, k.pct, k.count
                                    ));
                                }
                            }

                            if !dead.is_empty() {
                                summary.push_str(&format!("\nDead Keys ({}): {}\n", dead.len(), dead.join(", ")));
                            }

                            stats_bridge.set_stats_summary(SharedString::from(summary));

                            if window.global::<KeymapBridge>().get_heatmap_enabled() {
                                for i in 0..keycap_model.row_count() {
                                    let kp = &keys_guard[i];
                                    let row = kp.row as usize;
                                    let col = kp.col as usize;
                                    let count = heatmap_data
                                        .get(row)
                                        .and_then(|r| r.get(col))
                                        .copied()
                                        .unwrap_or(0);
                                    let value = if max_val > 0 {
                                        count as f32 / max_val as f32
                                    } else {
                                        0.0
                                    };
                                    let mut item = keycap_model.row_data(i).unwrap();
                                    item.color = heatmap_color(value);
                                    item.sublabel = SharedString::from(format!("{}", count));
                                    keycap_model.set_row_data(i, item);
                                }
                            }

                            window.global::<AppState>().set_status_text("Stats loaded".into());
                        }
                        BgMsg::MacroListData(entries) => {
                            *macro_entries.borrow_mut() = entries.clone();
                            let list = build_macro_list(&entries);
                            let list_model = Rc::new(VecModel::from(list));
                            window.global::<MacroBridge>().set_macros(ModelRc::from(list_model));
                            window.global::<AppState>().set_status_text(
                                SharedString::from(format!("{} macros loaded", entries.len()))
                            );
                        }
                        BgMsg::StatusMsg(msg) => {
                            window.global::<AppState>().set_status_text(SharedString::from(msg));
                        }
                        BgMsg::Notification(msg) => {
                            let app = window.global::<AppState>();
                            app.set_notification_text(SharedString::from(&msg));
                            app.set_notification_visible(true);
                            app.set_status_text(SharedString::from(msg));
                        }
                        BgMsg::OtaProgress(progress, status) => {
                            let sb = window.global::<SettingsBridge>();
                            sb.set_ota_progress(progress);
                            sb.set_ota_status(SharedString::from(status));
                            if progress >= 1.0 {
                                sb.set_ota_in_progress(false);
                            }
                        }
                        BgMsg::FlashProgress(progress, status) => {
                            let sb = window.global::<SettingsBridge>();
                            sb.set_flash_progress(progress);
                            sb.set_flash_status(SharedString::from(status));
                        }
                        BgMsg::Wpm(wpm) => {
                            window.global::<AppState>().set_wpm(wpm as i32);
                        }
                        BgMsg::PortList(ports) => {
                            let port_model: Vec<PortInfo> = ports.iter().map(|(name, path)| {
                                PortInfo {
                                    name: SharedString::from(name.as_str()),
                                    path: SharedString::from(path.as_str()),
                                }
                            }).collect();
                            let names_model: Vec<SharedString> = ports.iter()
                                .map(|(name, _)| SharedString::from(name.as_str()))
                                .collect();
                            let bridge = window.global::<ConnectionBridge>();
                            bridge.set_ports(ModelRc::from(Rc::new(VecModel::from(port_model))));
                            bridge.set_port_names(ModelRc::from(Rc::new(VecModel::from(names_model))));
                        }
                        BgMsg::TamaData(hunger, happiness, energy, health) => {
                            let bridge = window.global::<AdvancedBridge>();
                            bridge.set_tama_hunger(hunger);
                            bridge.set_tama_happiness(happiness);
                            bridge.set_tama_energy(energy);
                            bridge.set_tama_health(health);
                        }
                        BgMsg::AutoShiftData(enabled, timeout) => {
                            let bridge = window.global::<AdvancedBridge>();
                            bridge.set_autoshift_enabled(enabled);
                            bridge.set_autoshift_timeout(timeout);
                        }
                    }
                }
            },
        );

        // Keep timer alive
        let _keep_timer = timer;
        window.run().unwrap();
    }
}
