mod logic;

slint::include_modules!();

use logic::keycode;
use logic::layout::KeycapPos;
use logic::serial::SerialManager;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::rc::Rc;

/// Sender wrapper that wakes the Slint event loop after each send.
/// This eliminates the need for a polling timer.
#[derive(Clone)]
struct UiSender {
    tx: mpsc::Sender<BgMsg>,
    /// Sending a no-op invoke_from_event_loop wakes the event loop,
    /// which will then process the pending message via a zero-delay timer.
    _wake: Arc<dyn Fn() + Send + Sync>,
}

impl UiSender {
    fn new(tx: mpsc::Sender<BgMsg>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { tx, _wake: wake }
    }
    fn send(&self, msg: BgMsg) {
        let _ = self.tx.send(msg);
        (self._wake)();
    }
}

/// Build the full list of keycode entries for the key selector, grouped by category.
/// Entries with code=-1 are section headers.
fn build_keycode_entries() -> Vec<KeycodeEntry> {
    let mut e = Vec::new();

    // Letters A-Z (0x04 - 0x1D)
    push_header(&mut e, "Letters");
    for code in 0x04u8..=0x1D {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Letters");
    }

    // Numbers 0-9 (0x1E - 0x27)
    push_header(&mut e, "Numbers");
    for code in 0x1Eu8..=0x27 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Numbers");
    }

    // Modifiers (0xE0 - 0xE7)
    push_header(&mut e, "Modifiers");
    for code in 0xE0u8..=0xE7 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Modifiers");
    }

    // Navigation
    push_header(&mut e, "Navigation");
    for code in [0x28u8, 0x29, 0x2A, 0x2B, 0x2C, 0x39, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52] {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Navigation");
    }

    // F-Keys (F1-F24)
    push_header(&mut e, "F-Keys");
    for code in 0x3Au8..=0x45 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "F-Keys");
    }
    for code in 0x68u8..=0x73 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "F-Keys");
    }

    // Punctuation
    push_header(&mut e, "Punctuation");
    for code in 0x2Du8..=0x38 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Punctuation");
    }

    // Layers - MO, TO, OSL
    push_header(&mut e, "Layers");
    for layer in 0..10 {
        let code = ((layer + 1) << 8) as i32;
        push_entry(&mut e, code, &format!("MO {}", layer), "Layers");
    }
    for layer in 0..10 {
        let code = ((layer + 0x0B) << 8) as i32;
        push_entry(&mut e, code, &format!("TO {}", layer), "Layers");
    }
    for layer in 0..10 {
        let code = (0x3100 + layer) as i32;
        push_entry(&mut e, code, &format!("OSL {}", layer), "Layers");
    }

    // Special
    push_header(&mut e, "Special");
    let specials: &[(u16, &str)] = &[
        (0x0000, "None"), (0x3200, "Caps Word"), (0x3300, "Repeat"),
        (0x3400, "Leader"), (0x3500, "Feed"), (0x3600, "Play"),
        (0x3700, "Sleep"), (0x3800, "Meds"), (0x3900, "GEsc"),
        (0x3A00, "Layer Lock"), (0x3C00, "AS Toggle"),
    ];
    for &(code, label) in specials {
        push_entry(&mut e, code as i32, label, "Special");
    }

    // One-Shot Mod
    push_header(&mut e, "One-Shot Mod");
    let osm_mods: &[(u8, &str)] = &[
        (0x01, "OSM Ctrl"), (0x02, "OSM Shift"), (0x04, "OSM Alt"),
        (0x08, "OSM GUI"), (0x10, "OSM RCtrl"), (0x20, "OSM RShift"),
        (0x40, "OSM RAlt"), (0x80, "OSM RGUI"),
    ];
    for &(mod_mask, label) in osm_mods {
        push_entry(&mut e, 0x3000 + mod_mask as i32, label, "One-Shot Mod");
    }

    // Bluetooth
    push_header(&mut e, "Bluetooth");
    let bt: &[(u16, &str)] = &[
        (0x2900, "BT Next"), (0x2A00, "BT Prev"), (0x2B00, "BT Pair"),
        (0x2C00, "BT Disc"), (0x2E00, "USB/BT"), (0x2F00, "BT On/Off"),
    ];
    for &(code, label) in bt {
        push_entry(&mut e, code as i32, label, "Bluetooth");
    }

    // Media
    push_header(&mut e, "Media");
    let media: &[(u8, &str)] = &[
        (0x7F, "Mute"), (0x80, "Vol Up"), (0x81, "Vol Down"),
    ];
    for &(code, label) in media {
        push_entry(&mut e, code as i32, label, "Media");
    }

    // Numpad
    push_header(&mut e, "Numpad");
    for code in 0x53u8..=0x63 {
        push_entry(&mut e, code as i32, &keycode::hid_key_name(code), "Numpad");
    }

    // Macros M1-M20
    push_header(&mut e, "Macros");
    for idx in 1..=20 {
        let code = ((0x14 + idx) << 8) as i32;
        push_entry(&mut e, code, &format!("M{}", idx), "Macros");
    }

    e
}

fn push_header(entries: &mut Vec<KeycodeEntry>, name: &str) {
    entries.push(KeycodeEntry {
        code: -1,
        label: SharedString::from(name),
        category: SharedString::from(name),
    });
}

fn push_entry(entries: &mut Vec<KeycodeEntry>, code: i32, label: &str, category: &str) {
    entries.push(KeycodeEntry {
        code,
        label: SharedString::from(label),
        category: SharedString::from(category),
    });
}

/// Filter keycode entries by search text (case-insensitive).
/// Preserves section headers if the section has at least one matching entry.
fn filter_keycode_entries(all: &[KeycodeEntry], filter: &str) -> Vec<KeycodeEntry> {
    if filter.is_empty() {
        return all.to_vec();
    }
    let lower = filter.to_lowercase();
    let mut result = Vec::new();
    let mut i = 0;
    while i < all.len() {
        if all[i].code == -1 {
            // This is a section header. Collect all entries in this section.
            let header_idx = i;
            i += 1;
            let mut section_entries = Vec::new();
            while i < all.len() && all[i].code != -1 {
                if all[i].label.to_lowercase().contains(&lower) {
                    section_entries.push(all[i].clone());
                }
                i += 1;
            }
            if !section_entries.is_empty() {
                result.push(all[header_idx].clone());
                result.extend(section_entries);
            }
        } else {
            i += 1;
        }
    }
    result
}

// Messages from background serial thread to UI
enum BgMsg {
    Connected(String, String, Vec<String>, Vec<Vec<u16>>), // port, fw_version, layer_names, keymap
    LayoutJson(Vec<logic::layout::KeycapPos>), // physical layout received from firmware
    ConnectError(String),
    Keymap(Vec<Vec<u16>>),
    LayerNames(Vec<String>),
    Disconnected,
    TapDanceData(Vec<[u16; 4]>),
    ComboData(Vec<logic::parsers::ComboEntry>),
    LeaderData(Vec<logic::parsers::LeaderEntry>),
    KoData(Vec<[u8; 4]>),           // [trigger_key, trigger_mod, result_key, result_mod]
    BtData(Vec<String>),            // raw BT status lines from parse_bt_binary
    StatsData(Vec<Vec<u32>>, u32),  // heatmap data, max_value
    MacroListData(Vec<logic::parsers::MacroEntry>),
    Wpm(u16),
    TamaData(i32, i32, i32, i32),     // hunger, happiness, energy, health
    AutoShiftData(bool, i32),          // enabled, timeout_ms
    PortList(Vec<(String, String)>),   // (display_name, path)
    StatusMsg(String),
    Notification(String),
    OtaProgress(f32, String),
    FlashProgress(f32, String),
}

/// Interpolate a heatmap color from cold (blue) to hot (red).
/// value is 0.0..1.0
fn heatmap_color(value: f32) -> slint::Color {
    let r = (value * 255.0).min(255.0) as u8;
    let g = ((1.0 - (value - 0.5).abs() * 2.0) * 255.0).max(0.0) as u8;
    let b = ((1.0 - value) * 255.0).min(255.0) as u8;
    slint::Color::from_argb_u8(255, r, g, b)
}

fn build_keycap_model(keys: &[KeycapPos]) -> Rc<VecModel<KeycapData>> {
    let keycaps: Vec<KeycapData> = keys
        .iter()
        .enumerate()
        .map(|(idx, kp)| KeycapData {
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
        })
        .collect();
    Rc::new(VecModel::from(keycaps))
}

fn build_layer_model(names: &[String]) -> Rc<VecModel<LayerInfo>> {
    let layers: Vec<LayerInfo> = names
        .iter()
        .enumerate()
        .map(|(i, name)| LayerInfo {
            index: i as i32,
            name: SharedString::from(name.as_str()),
            active: i == 0,
        })
        .collect();
    Rc::new(VecModel::from(layers))
}

/// Update keycap labels from keymap data (row×col → keycode → label)
fn update_keycap_labels(
    keycap_model: &VecModel<KeycapData>,
    keys: &[KeycapPos],
    keymap: &[Vec<u16>],
    layout: &logic::layout_remap::KeyboardLayout,
) {
    for i in 0..keycap_model.row_count() {
        let mut item = keycap_model.row_data(i).unwrap();
        let kp = &keys[i];
        let row = kp.row as usize;
        let col = kp.col as usize;

        if row < keymap.len() && col < keymap[row].len() {
            let code = keymap[row][col];
            let decoded = keycode::decode_keycode(code);
            let remapped = logic::layout_remap::remap_key_label(layout, &decoded);
            let label = remapped.unwrap_or(&decoded).to_string();
            item.keycode = code as i32;
            item.label = SharedString::from(label);
            item.sublabel = if decoded != format!("0x{:04X}", code) {
                SharedString::default()
            } else {
                SharedString::from(format!("0x{:04X}", code))
            };
        }
        keycap_model.set_row_data(i, item);
    }
}

/// Build MacroStepInfo items from in-memory step data for the Slint model.
fn build_macro_step_infos(steps: &[(String, u8, u32)]) -> Vec<MacroStepInfo> {
    steps
        .iter()
        .map(|(action, kc, delay)| {
            let label = if action == "delay" {
                format!("{} ms", delay)
            } else {
                keycode::hid_key_name(*kc)
            };
            MacroStepInfo {
                action_type: SharedString::from(action.as_str()),
                keycode: *kc as i32,
                label: SharedString::from(label),
                delay_ms: *delay as i32,
            }
        })
        .collect()
}

/// Build MacroInfo list items from parsed macro entries.
fn build_macro_list(entries: &[logic::parsers::MacroEntry]) -> Vec<MacroInfo> {
    entries
        .iter()
        .map(|e| MacroInfo {
            slot: e.slot as i32,
            name: SharedString::from(e.name.as_str()),
            steps: e.steps.len() as i32,
        })
        .collect()
}

/// Convert firmware MacroStep entries into our in-memory edit format.
/// Firmware format: keycode=0xFF means delay (modifier*10 ms),
/// otherwise modifier is a bit field: bit0=press, bit1=release.
/// If modifier==0x01 => press, 0x02 => release, 0x03 => tap (press+release).
fn firmware_steps_to_edit(steps: &[logic::parsers::MacroStep]) -> Vec<(String, u8, u32)> {
    steps
        .iter()
        .map(|s| {
            if s.is_delay() {
                ("delay".to_string(), 0, s.delay_ms())
            } else {
                let action = match s.modifier {
                    0x01 => "press",
                    0x02 => "release",
                    _ => "tap", // 0x03 or default
                };
                (action.to_string(), s.keycode, 0)
            }
        })
        .collect()
}

/// Convert in-memory edit steps back to firmware hex format "kc:mod,kc:mod,..."
fn edit_steps_to_hex(steps: &[(String, u8, u32)]) -> String {
    steps
        .iter()
        .map(|(action, kc, delay)| {
            if action == "delay" {
                // Delay: keycode=0xFF, modifier = delay_ms / 10
                let ticks = (delay / 10).min(255) as u8;
                format!("{:02X}:{:02X}", 0xFF, ticks)
            } else {
                let modifier = match action.as_str() {
                    "press" => 0x01u8,
                    "release" => 0x02u8,
                    _ => 0x03u8, // tap
                };
                format!("{:02X}:{:02X}", kc, modifier)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

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
    // Wrap sender: each send() also wakes the Slint event loop via invoke_from_event_loop
    let wake_fn: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {
        let _ = slint::invoke_from_event_loop(|| {
            // no-op: just waking the event loop so the single-shot timer fires
        });
    });
    let bg_tx = UiSender::new(raw_tx, wake_fn);

    // Current state
    let current_keymap: Rc<std::cell::RefCell<Vec<Vec<u16>>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let current_layer: Rc<std::cell::Cell<usize>> = Rc::new(std::cell::Cell::new(0));
    let saved_settings = logic::settings::load();
    let keyboard_layout: Rc<std::cell::RefCell<logic::layout_remap::KeyboardLayout>> =
        Rc::new(std::cell::RefCell::new(logic::layout_remap::KeyboardLayout::from_name(&saved_settings.keyboard_layout)));

    // Macro editor state: current steps being edited (shared across callbacks)
    // Each entry: (action_type, keycode, delay_ms) where action_type is "press"/"release"/"tap"/"delay"
    let macro_steps: Rc<std::cell::RefCell<Vec<(String, u8, u32)>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    // Full macro list from firmware (for building the list model)
    let macro_entries: Rc<std::cell::RefCell<Vec<logic::parsers::MacroEntry>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));

    // OTA / Flash file paths (shared with callbacks, need Arc<Mutex> for thread safety)
    let ota_firmware_path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let flash_firmware_path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

    // --- Key selector setup ---
    let all_keycode_entries = build_keycode_entries();
    let keycode_model: Rc<VecModel<KeycodeEntry>> =
        Rc::new(VecModel::from(all_keycode_entries.clone()));

    {
        let ks_bridge = window.global::<KeySelectorBridge>();
        ks_bridge.set_entries(ModelRc::from(keycode_model.clone()));
    }

    // Key selector: search callback
    {
        let all_entries = all_keycode_entries.clone();
        let keycode_model = keycode_model.clone();
        window.global::<KeySelectorBridge>().on_search_changed(move |text| {
            let filtered = filter_keycode_entries(&all_entries, text.as_str());
            // Replace model contents
            let count = keycode_model.row_count();
            for _ in 0..count {
                keycode_model.remove(0);
            }
            for e in filtered {
                keycode_model.push(e);
            }
        });
    }

    // Key selector: select keycode callback
    {
        let keycap_model = keycap_model.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let current_layer = current_layer.clone();
        let keyboard_layout = keyboard_layout.clone();
        let serial = serial.clone();
        let all_entries = all_keycode_entries.clone();
        let keycode_model_sel = keycode_model.clone();
        let window_weak = window.as_weak();
        window.global::<KeySelectorBridge>().on_select_keycode(move |code| {
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };
            let ks = window.global::<KeySelectorBridge>();
            let key_idx = ks.get_editing_key_index();
            if key_idx < 0 { return; }

            let idx = key_idx as usize;
            let (row, col) = {
                let keys_guard = keys_arc.lock().unwrap();
                if idx >= keys_guard.len() { return; }
                (keys_guard[idx].row as usize, keys_guard[idx].col as usize)
            };

            // Update keymap in memory
            {
                let mut km = current_keymap.borrow_mut();
                if row < km.len() && col < km[row].len() {
                    km[row][col] = code as u16;
                }
            }

            // Update keycap label
            {
                let decoded = keycode::decode_keycode(code as u16);
                let layout = keyboard_layout.borrow();
                let remapped = logic::layout_remap::remap_key_label(&layout, &decoded);
                let label = remapped.unwrap_or(&decoded).to_string();
                let mut item = keycap_model.row_data(idx).unwrap();
                item.keycode = code;
                item.label = SharedString::from(&label);
                item.sublabel = if decoded != format!("0x{:04X}", code as u16) {
                    SharedString::default()
                } else {
                    SharedString::from(format!("0x{:04X}", code as u16))
                };
                keycap_model.set_row_data(idx, item);
            }

            // Update selected key label
            window.global::<KeymapBridge>().set_selected_key_label(
                SharedString::from(keycode::decode_keycode(code as u16)),
            );

            // Send change to keyboard via serial
            let layer = current_layer.get() as u8;
            let serial = serial.clone();
            let r = row as u8;
            let c = col as u8;
            let keycode_val = code as u16;
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if let Err(e) = ser.set_key(layer, r, c, keycode_val) {
                    eprintln!("Failed to send key change: {}", e);
                }
            });

            // Close modal and reset search + entries
            ks.set_active_modal(ModalKind::None);
            ks.set_search_filter(SharedString::default());
            let count = keycode_model_sel.row_count();
            for _ in 0..count {
                keycode_model_sel.remove(0);
            }
            for e in &all_entries {
                keycode_model_sel.push(e.clone());
            }
        });
    }

    // Key selector: cancel callback
    {
        let window_weak = window.as_weak();
        let all_entries = all_keycode_entries.clone();
        let keycode_model = keycode_model.clone();
        window.global::<KeySelectorBridge>().on_cancel(move || {
            if let Some(w) = window_weak.upgrade() {
                let ks = w.global::<KeySelectorBridge>();
                ks.set_active_modal(ModalKind::None);
                ks.set_search_filter(SharedString::default());
            }
            // Reset entries to unfiltered
            let count = keycode_model.row_count();
            for _ in 0..count {
                keycode_model.remove(0);
            }
            for e in &all_entries {
                keycode_model.push(e.clone());
            }
        });
    }

    // Key selector: apply MT (mod-tap)
    {
        let window_weak = window.as_weak();
        window.global::<KeySelectorBridge>().on_apply_mt(move || {
            if let Some(w) = window_weak.upgrade() {
                let ks = w.global::<KeySelectorBridge>();
                let mod_nibble = ks.get_mt_mod() & 0x0F;
                let key = ks.get_mt_key() & 0xFF;
                let code = 0x5000 | (mod_nibble << 8) | key;
                ks.invoke_select_keycode(code);
            }
        });
    }

    // Key selector: apply LT (layer-tap)
    {
        let window_weak = window.as_weak();
        window.global::<KeySelectorBridge>().on_apply_lt(move || {
            if let Some(w) = window_weak.upgrade() {
                let ks = w.global::<KeySelectorBridge>();
                let layer = ks.get_lt_layer() & 0x0F;
                let key = ks.get_lt_key() & 0xFF;
                let code = 0x4000 | (layer << 8) | key;
                ks.invoke_select_keycode(code);
            }
        });
    }

    // Key selector: apply hex
    {
        let window_weak = window.as_weak();
        window.global::<KeySelectorBridge>().on_apply_hex(move || {
            if let Some(w) = window_weak.upgrade() {
                let ks = w.global::<KeySelectorBridge>();
                let hex_str = ks.get_hex_input().to_string();
                let hex_str = hex_str.trim().trim_start_matches("0x").trim_start_matches("0X");
                if let Ok(code) = u16::from_str_radix(hex_str, 16) {
                    ks.invoke_select_keycode(code as i32);
                }
            }
        });
    }

    // --- StatsBridge: refresh-stats ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<StatsBridge>().on_refresh_stats(move || {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Loading key statistics...".into());
            }
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::KEYSTATS_BIN, &[]) {
                        Ok(resp) => {
                            let (data, max_val) = logic::parsers::parse_keystats_binary(&resp.payload);
                            let _ = tx.send(BgMsg::StatsData(data, max_val));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Stats error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_KEYSTATS) {
                        Ok(lines) => {
                            let (data, max_val) = logic::parsers::parse_heatmap_lines(&lines);
                            let _ = tx.send(BgMsg::StatsData(data, max_val));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Stats error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- StatsBridge: export-csv ---
    {
        let window_weak = window.as_weak();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
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

    // --- Populate port list on startup ---
    {
        let detailed = SerialManager::list_ports_detailed();
        let port_names: Vec<SharedString> = detailed.iter().map(|(d, _)| SharedString::from(d.as_str())).collect();
        let port_names_model = Rc::new(VecModel::from(port_names));
        window.global::<ConnectionBridge>().set_port_names(ModelRc::from(port_names_model));

        let port_infos: Vec<PortInfo> = detailed.iter().map(|(d, p)| PortInfo {
            name: SharedString::from(d.as_str()),
            path: SharedString::from(p.as_str()),
        }).collect();
        let port_infos_model = Rc::new(VecModel::from(port_infos));
        window.global::<ConnectionBridge>().set_ports(ModelRc::from(port_infos_model));
    }

    // --- Auto-connect on startup ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AppState>().set_status_text("Scanning ports...".into());
        window.global::<AppState>().set_connection(ConnectionState::Connecting);

        std::thread::spawn(move || {
            let mut ser = serial.lock().unwrap();
            match ser.auto_connect() {
                Ok(port_name) => {
                    let fw = ser.get_firmware_version().unwrap_or_default();
                    let names = ser.get_layer_names().unwrap_or_default();
                    let km = ser.get_keymap(0).unwrap_or_default();
                    let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));

                    // Try to get physical layout from firmware
                    if let Ok(json) = ser.get_layout_json() {
                        if let Ok(keys) = logic::layout::parse_json(&json) {
                            let _ = tx.send(BgMsg::LayoutJson(keys));
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(BgMsg::ConnectError(e));
                }
            }
        });
    }

    // --- Key selection callback ---
    {
        let keycap_model = keycap_model.clone();
        let window_weak = window.as_weak();
        keymap_bridge.on_select_key(move |key_index| {
            let idx = key_index as usize;
            if idx >= keycap_model.row_count() { return; }
            for i in 0..keycap_model.row_count() {
                let mut item = keycap_model.row_data(i).unwrap();
                let should_select = i == idx;
                if item.selected != should_select {
                    item.selected = should_select;
                    keycap_model.set_row_data(i, item);
                }
            }
            if let Some(w) = window_weak.upgrade() {
                let bridge = w.global::<KeymapBridge>();
                bridge.set_selected_key_index(key_index);
                let item = keycap_model.row_data(idx).unwrap();
                bridge.set_selected_key_label(item.label.clone());
            }
        });
    }

    // --- Layer switch callback ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let layer_model = layer_model.clone();
        let current_layer = current_layer.clone();
        let window_weak = window.as_weak();
        let window_weak_layer = window.as_weak();

        keymap_bridge.on_switch_layer(move |layer_index| {
            let idx = layer_index as usize;
            current_layer.set(idx);

            // Update active-layer property for UI reactivity
            if let Some(w) = window_weak_layer.upgrade() {
                w.global::<KeymapBridge>().set_active_layer(layer_index);
            }

            // Update layer model UI
            for i in 0..layer_model.row_count() {
                let mut item = layer_model.row_data(i).unwrap();
                let should_be_active = item.index == layer_index;
                if item.active != should_be_active {
                    item.active = should_be_active;
                    layer_model.set_row_data(i, item);
                }
            }

            // Load keymap for this layer
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text(SharedString::from(format!("Loading layer {}...", idx)));
            }
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                match ser.get_keymap(idx as u8) {
                    Ok(km) => { let _ = tx.send(BgMsg::Keymap(km)); }
                    Err(e) => { let _ = tx.send(BgMsg::ConnectError(e)); }
                }
            });
        });
    }

    // --- KeymapBridge: toggle-heatmap ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let keycap_model = keycap_model.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let keyboard_layout = keyboard_layout.clone();
        let window_weak = window.as_weak();
        keymap_bridge.on_toggle_heatmap(move |enabled| {
            if enabled {
                // Query keystats from firmware and apply heatmap colors to keycap_model
                if let Some(w) = window_weak.upgrade() {
                    w.global::<AppState>().set_status_text("Loading heatmap...".into());
                }
                let serial = serial.clone();
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let mut ser = serial.lock().unwrap();
                    if ser.v2 {
                        match ser.send_binary(logic::binary_protocol::cmd::KEYSTATS_BIN, &[]) {
                            Ok(resp) => {
                                let (data, max_val) = logic::parsers::parse_keystats_binary(&resp.payload);
                                let _ = tx.send(BgMsg::StatsData(data, max_val));
                            }
                            Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Heatmap error: {}", e))); }
                        }
                    } else {
                        match ser.query_command(logic::protocol::CMD_KEYSTATS) {
                            Ok(lines) => {
                                let (data, max_val) = logic::parsers::parse_heatmap_lines(&lines);
                                let _ = tx.send(BgMsg::StatsData(data, max_val));
                            }
                            Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Heatmap error: {}", e))); }
                        }
                    }
                });
            } else {
                // Reset keycap colors back to default gray
                let keys_guard = keys_arc.lock().unwrap();
                let km = current_keymap.borrow();
                for i in 0..keycap_model.row_count() {
                    let mut item = keycap_model.row_data(i).unwrap();
                    item.color = slint::Color::from_argb_u8(255, 0x44, 0x47, 0x5a);
                    item.sublabel = SharedString::default();
                    keycap_model.set_row_data(i, item);
                }
                // Re-apply labels from keymap (sublabels were overwritten with counts)
                if !km.is_empty() {
                    update_keycap_labels(&keycap_model, &keys_guard, &km, &keyboard_layout.borrow());
                }
                if let Some(w) = window_weak.upgrade() {
                    w.global::<AppState>().set_status_text("Heatmap disabled".into());
                }
            }
        });
    }

    // --- KeymapBridge: export-keymap ---
    {
        let current_keymap = current_keymap.clone();
        let current_layer = current_layer.clone();
        let window_weak = window.as_weak();
        keymap_bridge.on_export_keymap(move || {
            let km = current_keymap.borrow();
            let layer = current_layer.get();
            if km.is_empty() {
                if let Some(w) = window_weak.upgrade() {
                    w.global::<AppState>().set_status_text("No keymap data to export".into());
                }
                return;
            }

            // Get layer name from bridge
            let layer_name = if let Some(w) = window_weak.upgrade() {
                let layers_model = w.global::<KeymapBridge>().get_layers();
                if (layer as usize) < layers_model.row_count() {
                    layers_model.row_data(layer as usize)
                        .map(|l| l.name.to_string())
                        .unwrap_or_else(|| format!("Layer {}", layer))
                } else {
                    format!("Layer {}", layer)
                }
            } else {
                format!("Layer {}", layer)
            };

            // Build JSON: {"layer": N, "layer_name": "...", "rows": [[keycode, ...], ...]}
            let export = serde_json::json!({
                "layer": layer,
                "layer_name": layer_name,
                "rows": *km,
            });
            let json = serde_json::to_string_pretty(&export).unwrap_or_default();

            let dialog = rfd::FileDialog::new()
                .set_title("Export Keymap")
                .add_filter("JSON", &["json"])
                .set_file_name(&format!("keymap_layer{}.json", layer));

            if let Some(path) = dialog.save_file() {
                match std::fs::write(&path, &json) {
                    Ok(_) => {
                        if let Some(w) = window_weak.upgrade() {
                            w.global::<AppState>().set_notification_text(
                                SharedString::from(format!("Keymap exported to {}", path.display()))
                            );
                            w.global::<AppState>().set_notification_visible(true);
                            w.global::<AppState>().set_status_text(
                                SharedString::from(format!("Exported layer {} to {}", layer, path.display()))
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

    // --- KeymapBridge: import-keymap ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let current_layer = current_layer.clone();
        keymap_bridge.on_import_keymap(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            let layer = current_layer.get() as u8;
            std::thread::spawn(move || {
                let dialog = rfd::FileDialog::new()
                    .set_title("Import Keymap")
                    .add_filter("JSON", &["json"])
                    .pick_file();

                let path = match dialog {
                    Some(p) => p,
                    None => {
                        let _ = tx.send(BgMsg::StatusMsg("Import cancelled".into()));
                        return;
                    }
                };

                let contents = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(BgMsg::StatusMsg(format!("Read error: {}", e)));
                        return;
                    }
                };

                let parsed: serde_json::Value = match serde_json::from_str(&contents) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(BgMsg::StatusMsg(format!("JSON parse error: {}", e)));
                        return;
                    }
                };

                // Determine which layer to import into:
                // Use "layer" field from JSON if present, otherwise use current layer
                let target_layer = parsed.get("layer")
                    .and_then(|v| v.as_u64())
                    .map(|l| l as u8)
                    .unwrap_or(layer);

                let rows = match parsed.get("rows").and_then(|v| v.as_array()) {
                    Some(arr) => arr,
                    None => {
                        let _ = tx.send(BgMsg::StatusMsg("Invalid keymap JSON: missing 'rows'".into()));
                        return;
                    }
                };

                let mut ser = serial.lock().unwrap();
                if !ser.connected {
                    let _ = tx.send(BgMsg::StatusMsg("Not connected".into()));
                    return;
                }

                let _ = tx.send(BgMsg::StatusMsg(format!("Importing keymap to layer {}...", target_layer)));

                for (row_idx, row_val) in rows.iter().enumerate() {
                    let cols = match row_val.as_array() {
                        Some(c) => c,
                        None => continue,
                    };
                    for (col_idx, code_val) in cols.iter().enumerate() {
                        let code = code_val.as_u64().unwrap_or(0) as u16;
                        if let Err(e) = ser.set_key(target_layer, row_idx as u8, col_idx as u8, code) {
                            let _ = tx.send(BgMsg::StatusMsg(format!("Import error at R{}C{}: {}", row_idx, col_idx, e)));
                            return;
                        }
                    }
                }

                // Reload keymap for the target layer
                match ser.get_keymap(target_layer) {
                    Ok(km) => { let _ = tx.send(BgMsg::Keymap(km)); }
                    Err(_) => {}
                }
                let _ = tx.send(BgMsg::Notification("Keymap imported successfully".into()));
            });
        });
    }

    // --- KeymapBridge: rename-layer ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        keymap_bridge.on_rename_layer(move |layer_index, new_name| {
            let name = new_name.to_string();
            if name.is_empty() {
                let _ = tx.send(BgMsg::StatusMsg("Layer name cannot be empty".into()));
                return;
            }
            let serial = serial.clone();
            let tx = tx.clone();
            let layer = layer_index as u8;
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if !ser.connected {
                    let _ = tx.send(BgMsg::StatusMsg("Not connected".into()));
                    return;
                }

                if ser.v2 {
                    // Binary v2: LAYER_NAME command, payload = [layer_index, name_bytes...]
                    let mut payload = vec![layer];
                    payload.extend_from_slice(name.as_bytes());
                    match ser.send_binary(logic::binary_protocol::cmd::LAYER_NAME, &payload) {
                        Ok(_) => {
                            let _ = tx.send(BgMsg::Notification(format!("Layer {} renamed to '{}'", layer, name)));
                        }
                        Err(e) => {
                            let _ = tx.send(BgMsg::StatusMsg(format!("Rename error: {}", e)));
                            return;
                        }
                    }
                } else {
                    // Legacy: LAYOUTNAME0:MyLayer
                    let cmd = logic::protocol::cmd_set_layer_name(layer, &name);
                    if let Err(e) = ser.send_command(&cmd) {
                        let _ = tx.send(BgMsg::StatusMsg(format!("Rename error: {}", e)));
                        return;
                    }
                    let _ = tx.send(BgMsg::Notification(format!("Layer {} renamed to '{}'", layer, name)));
                }

                // Reload layer names
                match ser.get_layer_names() {
                    Ok(names) => { let _ = tx.send(BgMsg::LayerNames(names)); }
                    Err(_) => {}
                }
            });
        });
    }

    // --- Connect/Disconnect callbacks ---
    {
        let serial_c = serial.clone();
        let tx_c = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<ConnectionBridge>().on_connect(move || {
            // Read selected port path from the bridge
            let selected_path = if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Connecting...".into());
                w.global::<AppState>().set_connection(ConnectionState::Connecting);
                let bridge = w.global::<ConnectionBridge>();
                let idx = bridge.get_selected_port_index();
                if idx >= 0 {
                    let ports_model = bridge.get_ports();
                    if (idx as usize) < ports_model.row_count() {
                        ports_model.row_data(idx as usize).map(|pi| pi.path.to_string()).unwrap_or_default()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let serial = serial_c.clone();
            let tx = tx_c.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                let connect_result = if selected_path.is_empty() {
                    // No port selected: auto-connect by VID/PID
                    ser.auto_connect()
                } else {
                    // Connect to the specific selected port
                    ser.connect(&selected_path).map(|_| selected_path.clone())
                };
                match connect_result {
                    Ok(port_name) => {
                        let fw = ser.get_firmware_version().unwrap_or_default();
                        let names = ser.get_layer_names().unwrap_or_default();
                        let km = ser.get_keymap(0).unwrap_or_default();
                        let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));

                        if let Ok(json) = ser.get_layout_json() {
                            if let Ok(keys) = logic::layout::parse_json(&json) {
                                let _ = tx.send(BgMsg::LayoutJson(keys));
                            }
                        }
                    }
                    Err(e) => { let _ = tx.send(BgMsg::ConnectError(e)); }
                }
            });
        });
    }

    {
        let serial_d = serial.clone();
        let tx_d = bg_tx.clone();
        window.global::<ConnectionBridge>().on_disconnect(move || {
            let mut ser = serial_d.lock().unwrap();
            ser.disconnect();
            let _ = tx_d.send(BgMsg::Disconnected);
        });
    }

    // --- Refresh ports callback ---
    {
        let tx = bg_tx.clone();
        window.global::<ConnectionBridge>().on_refresh_ports(move || {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let ports = SerialManager::list_ports_detailed();
                let _ = tx.send(BgMsg::PortList(ports));
            });
        });
    }

    // --- AdvancedBridge: refresh-all ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<AdvancedBridge>().on_refresh_all(move || {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Loading advanced data...".into());
            }
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();

                // Query tap dance data
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::TD_LIST, &[]) {
                        Ok(resp) => {
                            let td = logic::parsers::parse_td_binary(&resp.payload);
                            let _ = tx.send(BgMsg::TapDanceData(td));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("TD error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_TAP_DANCE) {
                        Ok(lines) => {
                            let td = logic::parsers::parse_td_lines(&lines);
                            let _ = tx.send(BgMsg::TapDanceData(td));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("TD error: {}", e))); }
                    }
                }

                // Query combo data
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::COMBO_LIST, &[]) {
                        Ok(resp) => {
                            let combos = logic::parsers::parse_combo_binary(&resp.payload);
                            let _ = tx.send(BgMsg::ComboData(combos));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Combo error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_COMBOS) {
                        Ok(lines) => {
                            let combos = logic::parsers::parse_combo_lines(&lines);
                            let _ = tx.send(BgMsg::ComboData(combos));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Combo error: {}", e))); }
                    }
                }

                // Query leader data
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::LEADER_LIST, &[]) {
                        Ok(resp) => {
                            let leaders = logic::parsers::parse_leader_binary(&resp.payload);
                            let _ = tx.send(BgMsg::LeaderData(leaders));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Leader error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_LEADER) {
                        Ok(lines) => {
                            let leaders = logic::parsers::parse_leader_lines(&lines);
                            let _ = tx.send(BgMsg::LeaderData(leaders));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Leader error: {}", e))); }
                    }
                }

                // Query key override data
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::KO_LIST, &[]) {
                        Ok(resp) => {
                            let kos = logic::parsers::parse_ko_binary(&resp.payload);
                            let _ = tx.send(BgMsg::KoData(kos));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("KO error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_KEY_OVERRIDE) {
                        Ok(lines) => {
                            let kos = logic::parsers::parse_ko_lines(&lines);
                            let _ = tx.send(BgMsg::KoData(kos));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("KO error: {}", e))); }
                    }
                }

                // Query bluetooth data
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::BT_QUERY, &[]) {
                        Ok(resp) => {
                            let bt_lines = logic::parsers::parse_bt_binary(&resp.payload);
                            let _ = tx.send(BgMsg::BtData(bt_lines));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("BT error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_BT_STATUS) {
                        Ok(lines) => {
                            let _ = tx.send(BgMsg::BtData(lines));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("BT error: {}", e))); }
                    }
                }

                // Query autoshift data
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::AUTOSHIFT_TOGGLE, &[0xFF]) {
                        Ok(resp) => {
                            // Payload: [enabled:u8][timeout:u16 LE]
                            if resp.payload.len() >= 3 {
                                let enabled = resp.payload[0] != 0;
                                let timeout = u16::from_le_bytes([resp.payload[1], resp.payload[2]]) as i32;
                                let _ = tx.send(BgMsg::AutoShiftData(enabled, timeout));
                            }
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("AutoShift error: {}", e))); }
                    }
                }

                // Query tamagotchi data
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::TAMA_QUERY, &[]) {
                        Ok(resp) => {
                            let lines = logic::parsers::parse_tama_binary(&resp.payload);
                            // Parse the summary line: "TAMA: Lv1 hunger=75 happy=60 energy=90 health=80 keys=1234 enabled=1"
                            for line in &lines {
                                if line.starts_with("TAMA:") {
                                    let parse_val = |key: &str| -> i32 {
                                        line.find(key)
                                            .and_then(|i| line[i + key.len()..].split_whitespace().next())
                                            .and_then(|v| v.parse::<i32>().ok())
                                            .unwrap_or(0)
                                    };
                                    let hunger = parse_val("hunger=");
                                    let happiness = parse_val("happy=");
                                    let energy = parse_val("energy=");
                                    let health = parse_val("health=");
                                    let _ = tx.send(BgMsg::TamaData(hunger, happiness, energy, health));
                                }
                            }
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_TAMA) {
                        Ok(lines) => {
                            for line in &lines {
                                if line.starts_with("TAMA:") {
                                    let parse_val = |key: &str| -> i32 {
                                        line.find(key)
                                            .and_then(|i| line[i + key.len()..].split_whitespace().next())
                                            .and_then(|v| v.parse::<i32>().ok())
                                            .unwrap_or(0)
                                    };
                                    let hunger = parse_val("hunger=");
                                    let happiness = parse_val("happy=");
                                    let energy = parse_val("energy=");
                                    let health = parse_val("health=");
                                    let _ = tx.send(BgMsg::TamaData(hunger, happiness, energy, health));
                                }
                            }
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama error: {}", e))); }
                    }
                }

                let _ = tx.send(BgMsg::StatusMsg("Advanced data loaded".into()));
            });
        });
    }

    // --- AdvancedBridge: save-td ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<AdvancedBridge>().on_save_td(move |slot_index| {
            // Read current TD slot data from the model
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };
            let bridge = window.global::<AdvancedBridge>();
            let slots_model = bridge.get_tap_dance_slots();
            let idx = slot_index as usize;
            if idx >= slots_model.row_count() { return; }
            let slot = slots_model.row_data(idx).unwrap();

            let actions: [u16; 4] = [
                slot.tap1_code as u16,
                slot.tap2_code as u16,
                slot.tap3_code as u16,
                slot.hold_code as u16,
            ];

            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = logic::binary_protocol::td_set_payload(slot_index as u8, &actions);
                    match ser.send_binary(logic::binary_protocol::cmd::TD_SET, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("TD {} saved", slot_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("TD save error: {}", e))); }
                    }
                } else {
                    let cmd = format!("TDSET {};{:02X},{:02X},{:02X},{:02X}",
                        slot_index, actions[0], actions[1], actions[2], actions[3]);
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("TD {} saved", slot_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("TD save error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- AdvancedBridge: delete-combo ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_delete_combo(move |combo_index| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = vec![combo_index as u8];
                    match ser.send_binary(logic::binary_protocol::cmd::COMBO_DELETE, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Combo {} deleted", combo_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Combo delete error: {}", e))); }
                    }
                } else {
                    let cmd = logic::protocol::cmd_combodel(combo_index as u8);
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Combo {} deleted", combo_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Combo delete error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- AdvancedBridge: delete-leader ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_delete_leader(move |leader_index| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = vec![leader_index as u8];
                    match ser.send_binary(logic::binary_protocol::cmd::LEADER_DELETE, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Leader {} deleted", leader_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Leader delete error: {}", e))); }
                    }
                } else {
                    let cmd = logic::protocol::cmd_leaderdel(leader_index as u8);
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Leader {} deleted", leader_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Leader delete error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- AdvancedBridge: delete-ko ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_delete_ko(move |ko_index| {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = vec![ko_index as u8];
                    match ser.send_binary(logic::binary_protocol::cmd::KO_DELETE, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("KO {} deleted", ko_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("KO delete error: {}", e))); }
                    }
                } else {
                    let cmd = logic::protocol::cmd_kodel(ko_index as u8);
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("KO {} deleted", ko_index))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("KO delete error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- AdvancedBridge: bt-next ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_bt_next(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::BT_NEXT, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("BT Next".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("BT Next error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("BT NEXT");
                    let _ = tx.send(BgMsg::StatusMsg("BT Next".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: bt-prev ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_bt_prev(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::BT_PREV, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("BT Prev".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("BT Prev error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("BT PREV");
                    let _ = tx.send(BgMsg::StatusMsg("BT Prev".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: bt-pair ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_bt_pair(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::BT_PAIR, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("BT Pairing...".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("BT Pair error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("BT PAIR");
                    let _ = tx.send(BgMsg::StatusMsg("BT Pairing...".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: bt-disconnect ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_bt_disconnect(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::BT_DISCONNECT, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("BT Disconnected".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("BT Disconnect error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("BT DISCONNECT");
                    let _ = tx.send(BgMsg::StatusMsg("BT Disconnected".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: bt-toggle-usb-bt ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_bt_toggle_usb_bt(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    // BT_SWITCH with slot=0xFF acts as USB/BT toggle
                    match ser.send_binary(logic::binary_protocol::cmd::BT_SWITCH, &[0xFF]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("USB/BT toggled".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("USB/BT toggle error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("BT TOGGLE");
                    let _ = tx.send(BgMsg::StatusMsg("USB/BT toggled".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: save-autoshift ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<AdvancedBridge>().on_save_autoshift(move || {
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };
            let bridge = window.global::<AdvancedBridge>();
            let enabled = bridge.get_autoshift_enabled();
            let timeout = bridge.get_autoshift_timeout() as u16;
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = vec![
                        if enabled { 1u8 } else { 0u8 },
                        (timeout & 0xFF) as u8,
                        (timeout >> 8) as u8,
                    ];
                    match ser.send_binary(logic::binary_protocol::cmd::AUTOSHIFT_TOGGLE, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Auto Shift: {} timeout={}ms", if enabled { "ON" } else { "OFF" }, timeout))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("AutoShift error: {}", e))); }
                    }
                } else {
                    let cmd = if enabled {
                        format!("AUTOSHIFT ON {}", timeout)
                    } else {
                        "AUTOSHIFT OFF".to_string()
                    };
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Auto Shift: {} timeout={}ms", if enabled { "ON" } else { "OFF" }, timeout))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("AutoShift error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- AdvancedBridge: save-tri-layer ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<AdvancedBridge>().on_save_tri_layer(move || {
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };
            let bridge = window.global::<AdvancedBridge>();
            let l1 = bridge.get_tri_layer1() as u8;
            let l2 = bridge.get_tri_layer2() as u8;
            let l3 = bridge.get_tri_layer3() as u8;
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = vec![l1, l2, l3];
                    match ser.send_binary(logic::binary_protocol::cmd::TRILAYER_SET, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tri-Layer set: {} + {} = {}", l1, l2, l3))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tri-Layer error: {}", e))); }
                    }
                } else {
                    let cmd = logic::protocol::cmd_trilayer(l1, l2, l3);
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tri-Layer set: {} + {} = {}", l1, l2, l3))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tri-Layer error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- AdvancedBridge: tama-feed ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_tama_feed(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::TAMA_FEED, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("Tama: Fed!".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama Feed error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("TAMA FEED");
                    let _ = tx.send(BgMsg::StatusMsg("Tama: Fed!".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: tama-play ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_tama_play(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::TAMA_PLAY, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("Tama: Played!".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama Play error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("TAMA PLAY");
                    let _ = tx.send(BgMsg::StatusMsg("Tama: Played!".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: tama-sleep ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_tama_sleep(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::TAMA_SLEEP, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("Tama: Sleeping...".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama Sleep error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("TAMA SLEEP");
                    let _ = tx.send(BgMsg::StatusMsg("Tama: Sleeping...".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: tama-meds ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_tama_meds(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::TAMA_MEDICINE, &[]) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg("Tama: Medicine given!".into())); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama Meds error: {}", e))); }
                    }
                } else {
                    let _ = ser.send_command("TAMA MEDS");
                    let _ = tx.send(BgMsg::StatusMsg("Tama: Medicine given!".into()));
                }
            });
        });
    }

    // --- AdvancedBridge: refresh-tama ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<AdvancedBridge>().on_refresh_tama(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::TAMA_QUERY, &[]) {
                        Ok(resp) => {
                            let lines = logic::parsers::parse_tama_binary(&resp.payload);
                            for line in &lines {
                                if line.starts_with("TAMA:") {
                                    let parse_val = |key: &str| -> i32 {
                                        line.find(key)
                                            .and_then(|i| line[i + key.len()..].split_whitespace().next())
                                            .and_then(|v| v.parse::<i32>().ok())
                                            .unwrap_or(0)
                                    };
                                    let hunger = parse_val("hunger=");
                                    let happiness = parse_val("happy=");
                                    let energy = parse_val("energy=");
                                    let health = parse_val("health=");
                                    let _ = tx.send(BgMsg::TamaData(hunger, happiness, energy, health));
                                }
                            }
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_TAMA) {
                        Ok(lines) => {
                            for line in &lines {
                                if line.starts_with("TAMA:") {
                                    let parse_val = |key: &str| -> i32 {
                                        line.find(key)
                                            .and_then(|i| line[i + key.len()..].split_whitespace().next())
                                            .and_then(|v| v.parse::<i32>().ok())
                                            .unwrap_or(0)
                                    };
                                    let hunger = parse_val("hunger=");
                                    let happiness = parse_val("happy=");
                                    let energy = parse_val("energy=");
                                    let health = parse_val("health=");
                                    let _ = tx.send(BgMsg::TamaData(hunger, happiness, energy, health));
                                }
                            }
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Tama error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- MacroBridge: refresh-macros ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_refresh_macros(move || {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Loading macros...".into());
            }
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    match ser.send_binary(logic::binary_protocol::cmd::LIST_MACROS, &[]) {
                        Ok(resp) => {
                            let entries = logic::parsers::parse_macros_binary(&resp.payload);
                            let _ = tx.send(BgMsg::MacroListData(entries));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro error: {}", e))); }
                    }
                } else {
                    match ser.query_command(logic::protocol::CMD_MACROS_TEXT) {
                        Ok(lines) => {
                            let entries = logic::parsers::parse_macro_lines(&lines);
                            let _ = tx.send(BgMsg::MacroListData(entries));
                        }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- MacroBridge: select-macro ---
    {
        let macro_entries = macro_entries.clone();
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_select_macro(move |slot| {
            let window = match window_weak.upgrade() {
                Some(w) => w,
                None => return,
            };
            let bridge = window.global::<MacroBridge>();
            bridge.set_selected_macro(slot);
            let entries = macro_entries.borrow();
            let found = entries.iter().find(|e| e.slot as i32 == slot);
            match found {
                Some(entry) => {
                    bridge.set_macro_name(SharedString::from(entry.name.as_str()));
                    let edit_steps = firmware_steps_to_edit(&entry.steps);
                    let infos = build_macro_step_infos(&edit_steps);
                    *macro_steps.borrow_mut() = edit_steps;
                    bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
                }
                None => {
                    bridge.set_macro_name(SharedString::default());
                    macro_steps.borrow_mut().clear();
                    bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::<MacroStepInfo>::from(vec![]))));
                }
            }
        });
    }

    // --- MacroBridge: new-macro ---
    {
        let macro_entries = macro_entries.clone();
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_new_macro(move || {
            let window = match window_weak.upgrade() { Some(w) => w, None => return };
            let bridge = window.global::<MacroBridge>();
            let entries = macro_entries.borrow();
            let used: Vec<u8> = entries.iter().map(|e| e.slot).collect();
            match (0u8..16).find(|s| !used.contains(s)) {
                Some(slot) => {
                    bridge.set_selected_macro(slot as i32);
                    bridge.set_macro_name(SharedString::from(format!("Macro{}", slot)));
                    macro_steps.borrow_mut().clear();
                    bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::<MacroStepInfo>::from(vec![]))));
                }
                None => {
                    window.global::<AppState>().set_status_text("All 16 macro slots are used".into());
                }
            }
        });
    }

    // --- MacroBridge: add-step ---
    {
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_add_step(move |action_type, keycode_or_delay| {
            let window = match window_weak.upgrade() { Some(w) => w, None => return };
            let bridge = window.global::<MacroBridge>();
            let action = action_type.to_string();
            let mut steps = macro_steps.borrow_mut();
            if action == "delay" {
                let delay_str = bridge.get_delay_input().to_string();
                let delay_ms: u32 = delay_str.trim().parse().unwrap_or(50);
                steps.push(("delay".to_string(), 0, delay_ms));
            } else {
                steps.push((action, keycode_or_delay as u8, 0));
            }
            let infos = build_macro_step_infos(&steps);
            bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
        });
    }

    // --- MacroBridge: remove-step ---
    {
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_remove_step(move |idx| {
            let window = match window_weak.upgrade() { Some(w) => w, None => return };
            let bridge = window.global::<MacroBridge>();
            let mut steps = macro_steps.borrow_mut();
            let i = idx as usize;
            if i < steps.len() { steps.remove(i); }
            let infos = build_macro_step_infos(&steps);
            bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
        });
    }

    // --- MacroBridge: move-step-up ---
    {
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_move_step_up(move |idx| {
            let window = match window_weak.upgrade() { Some(w) => w, None => return };
            let bridge = window.global::<MacroBridge>();
            let mut steps = macro_steps.borrow_mut();
            let i = idx as usize;
            if i > 0 && i < steps.len() { steps.swap(i, i - 1); }
            let infos = build_macro_step_infos(&steps);
            bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
        });
    }

    // --- MacroBridge: move-step-down ---
    {
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_move_step_down(move |idx| {
            let window = match window_weak.upgrade() { Some(w) => w, None => return };
            let bridge = window.global::<MacroBridge>();
            let mut steps = macro_steps.borrow_mut();
            let i = idx as usize;
            if i + 1 < steps.len() { steps.swap(i, i + 1); }
            let infos = build_macro_step_infos(&steps);
            bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::from(infos))));
        });
    }

    // --- MacroBridge: save-macro ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let macro_steps = macro_steps.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_save_macro(move || {
            let window = match window_weak.upgrade() { Some(w) => w, None => return };
            let bridge = window.global::<MacroBridge>();
            let slot = bridge.get_selected_macro();
            if slot < 0 { return; }
            let name = bridge.get_macro_name().to_string();
            let steps = macro_steps.borrow();
            let steps_hex = edit_steps_to_hex(&steps);
            let slot_u8 = slot as u8;
            window.global::<AppState>().set_status_text(SharedString::from(format!("Saving macro {}...", slot)));
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = logic::binary_protocol::macro_add_seq_payload(slot_u8, &name, &steps_hex);
                    match ser.send_binary(logic::binary_protocol::cmd::MACRO_ADD_SEQ, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro {} saved", slot))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro save error: {}", e))); }
                    }
                } else {
                    let cmd = logic::protocol::cmd_macroseq(slot_u8, &name, &steps_hex);
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro {} saved", slot))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro save error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- MacroBridge: delete-macro ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let macro_steps = macro_steps.clone();
        let macro_entries = macro_entries.clone();
        let window_weak = window.as_weak();
        window.global::<MacroBridge>().on_delete_macro(move |slot| {
            if slot < 0 { return; }
            let slot_u8 = slot as u8;
            macro_steps.borrow_mut().clear();
            { macro_entries.borrow_mut().retain(|e| e.slot != slot_u8); }
            if let Some(w) = window_weak.upgrade() {
                let bridge = w.global::<MacroBridge>();
                bridge.set_selected_macro(-1);
                bridge.set_macro_name(SharedString::default());
                bridge.set_current_steps(ModelRc::from(Rc::new(VecModel::<MacroStepInfo>::from(vec![]))));
                let entries = macro_entries.borrow();
                let list = build_macro_list(&entries);
                bridge.set_macros(ModelRc::from(Rc::new(VecModel::from(list))));
                w.global::<AppState>().set_status_text(SharedString::from(format!("Deleting macro {}...", slot)));
            }
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                if ser.v2 {
                    let payload = logic::binary_protocol::macro_delete_payload(slot_u8);
                    match ser.send_binary(logic::binary_protocol::cmd::MACRO_DELETE, &payload) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro {} deleted", slot))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro delete error: {}", e))); }
                    }
                } else {
                    let cmd = logic::protocol::cmd_macro_del(slot_u8);
                    match ser.send_command(&cmd) {
                        Ok(_) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro {} deleted", slot))); }
                        Err(e) => { let _ = tx.send(BgMsg::StatusMsg(format!("Macro delete error: {}", e))); }
                    }
                }
            });
        });
    }

    // --- SettingsBridge setup ---
    {
        // Populate available layouts
        let layout_names: Vec<SharedString> = logic::layout_remap::KeyboardLayout::all()
            .iter()
            .map(|l| SharedString::from(l.name()))
            .collect();
        let layout_model = Rc::new(VecModel::from(layout_names));
        window.global::<SettingsBridge>().set_available_layouts(ModelRc::from(layout_model));

        // Set initial selection to match saved setting
        let saved_layout_name = saved_settings.keyboard_layout.to_ascii_uppercase();
        let initial_idx = logic::layout_remap::KeyboardLayout::all()
            .iter()
            .position(|l| l.name() == saved_layout_name)
            .unwrap_or(0);
        window.global::<SettingsBridge>().set_selected_layout_index(initial_idx as i32);

        // Populate programming ports
        let prog_ports: Vec<SharedString> = SerialManager::list_ports()
            .into_iter()
            .map(|p| SharedString::from(p))
            .collect();
        let prog_ports_model = Rc::new(VecModel::from(prog_ports));
        window.global::<SettingsBridge>().set_prog_ports(ModelRc::from(prog_ports_model));
    }

    // --- SettingsBridge: change-layout ---
    {
        let keyboard_layout = keyboard_layout.clone();
        let keycap_model = keycap_model.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let window_weak = window.as_weak();
        window.global::<SettingsBridge>().on_change_layout(move |idx| {
            let all = logic::layout_remap::KeyboardLayout::all();
            let idx = idx as usize;
            if idx >= all.len() { return; }
            let new_layout = all[idx];
            *keyboard_layout.borrow_mut() = new_layout;

            // Save to settings
            let mut settings = logic::settings::load();
            settings.keyboard_layout = new_layout.name().to_string();
            logic::settings::save(&settings);

            // Re-render keycap labels with new layout
            let km = current_keymap.borrow();
            if !km.is_empty() {
                update_keycap_labels(&keycap_model, &keys_arc.lock().unwrap(), &km, &new_layout);
            }

            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text(
                    SharedString::from(format!("Layout: {}", new_layout.name()))
                );
            }
        });
    }

    // --- SettingsBridge: backup ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<SettingsBridge>().on_backup(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                // Read all layer keymaps from the keyboard
                let mut ser = serial.lock().unwrap();
                if !ser.connected {
                    let _ = tx.send(BgMsg::StatusMsg("Not connected".into()));
                    return;
                }

                let layer_names = ser.get_layer_names().unwrap_or_default();
                let num_layers = layer_names.len().max(1);
                let mut keymaps = Vec::new();
                for l in 0..num_layers {
                    match ser.get_keymap(l as u8) {
                        Ok(km) => keymaps.push(km),
                        Err(e) => {
                            let _ = tx.send(BgMsg::StatusMsg(format!("Backup error (layer {}): {}", l, e)));
                            return;
                        }
                    }
                }
                drop(ser); // release lock before file dialog

                let backup = serde_json::json!({
                    "layer_names": layer_names,
                    "keymaps": keymaps,
                });
                let json = serde_json::to_string_pretty(&backup).unwrap_or_default();

                let save_dialog = rfd::FileDialog::new()
                    .set_title("Save Backup")
                    .add_filter("JSON", &["json"])
                    .set_file_name("kase_backup.json")
                    .save_file();

                match save_dialog {
                    Some(path) => {
                        match std::fs::write(&path, &json) {
                            Ok(_) => {
                                let _ = tx.send(BgMsg::StatusMsg(format!("Backup saved to {}", path.display())));
                            }
                            Err(e) => {
                                let _ = tx.send(BgMsg::StatusMsg(format!("Backup write error: {}", e)));
                            }
                        }
                    }
                    None => {
                        let _ = tx.send(BgMsg::StatusMsg("Backup cancelled".into()));
                    }
                }
            });
        });
    }

    // --- SettingsBridge: restore ---
    {
        let serial = serial.clone();
        let tx = bg_tx.clone();
        window.global::<SettingsBridge>().on_restore(move || {
            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let open_dialog = rfd::FileDialog::new()
                    .set_title("Restore Backup")
                    .add_filter("JSON", &["json"])
                    .pick_file();

                let path = match open_dialog {
                    Some(p) => p,
                    None => {
                        let _ = tx.send(BgMsg::StatusMsg("Restore cancelled".into()));
                        return;
                    }
                };

                let contents = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(BgMsg::StatusMsg(format!("Read error: {}", e)));
                        return;
                    }
                };

                let parsed: serde_json::Value = match serde_json::from_str(&contents) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = tx.send(BgMsg::StatusMsg(format!("JSON parse error: {}", e)));
                        return;
                    }
                };

                let keymaps = match parsed.get("keymaps").and_then(|v| v.as_array()) {
                    Some(arr) => arr,
                    None => {
                        let _ = tx.send(BgMsg::StatusMsg("Invalid backup: missing keymaps".into()));
                        return;
                    }
                };

                let mut ser = serial.lock().unwrap();
                if !ser.connected {
                    let _ = tx.send(BgMsg::StatusMsg("Not connected".into()));
                    return;
                }

                for (layer_idx, layer_val) in keymaps.iter().enumerate() {
                    let rows = match layer_val.as_array() {
                        Some(r) => r,
                        None => continue,
                    };
                    for (row_idx, row_val) in rows.iter().enumerate() {
                        let cols = match row_val.as_array() {
                            Some(c) => c,
                            None => continue,
                        };
                        for (col_idx, code_val) in cols.iter().enumerate() {
                            let code = code_val.as_u64().unwrap_or(0) as u16;
                            if let Err(e) = ser.set_key(layer_idx as u8, row_idx as u8, col_idx as u8, code) {
                                let _ = tx.send(BgMsg::StatusMsg(format!("Restore error at L{}R{}C{}: {}", layer_idx, row_idx, col_idx, e)));
                                return;
                            }
                        }
                    }
                    let _ = tx.send(BgMsg::StatusMsg(format!("Restored layer {}/{}", layer_idx + 1, keymaps.len())));
                }

                // Reload layer 0 keymap
                match ser.get_keymap(0) {
                    Ok(km) => { let _ = tx.send(BgMsg::Keymap(km)); }
                    Err(_) => {}
                }
                let _ = tx.send(BgMsg::StatusMsg("Restore complete".into()));
            });
        });
    }

    // --- SettingsBridge: OTA select file ---
    {
        let ota_firmware_path = ota_firmware_path.clone();
        let window_weak = window.as_weak();
        window.global::<SettingsBridge>().on_ota_select_file(move || {
            let ota_firmware_path = ota_firmware_path.clone();
            let window_weak = window_weak.clone();
            std::thread::spawn(move || {
                let dialog = rfd::FileDialog::new()
                    .set_title("Select OTA Firmware")
                    .add_filter("Binary", &["bin"])
                    .pick_file();

                if let Some(path) = dialog {
                    let display = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.display().to_string());
                    *ota_firmware_path.lock().unwrap() = path.display().to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.global::<SettingsBridge>().set_ota_file_name(SharedString::from(&display));
                        }
                    });
                }
            });
        });
    }

    // --- SettingsBridge: OTA start ---
    {
        let ota_firmware_path = ota_firmware_path.clone();
        let serial = serial.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<SettingsBridge>().on_ota_start(move || {
            let fw_path = ota_firmware_path.lock().unwrap().clone();
            if fw_path.is_empty() { return; }

            if let Some(w) = window_weak.upgrade() {
                w.global::<SettingsBridge>().set_ota_in_progress(true);
                w.global::<SettingsBridge>().set_ota_progress(0.0);
                w.global::<SettingsBridge>().set_ota_status(SharedString::from("Reading firmware file..."));
            }

            let serial = serial.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let firmware = match std::fs::read(&fw_path) {
                    Ok(data) => data,
                    Err(e) => {
                        let _ = tx.send(BgMsg::OtaProgress(0.0, format!("File read error: {}", e)));
                        return;
                    }
                };

                let _ = tx.send(BgMsg::OtaProgress(0.02, format!("Firmware: {} KB", firmware.len() / 1024)));

                let mut ser = serial.lock().unwrap();
                if !ser.connected || !ser.v2 {
                    let _ = tx.send(BgMsg::OtaProgress(0.0, "Not connected (v2 required for OTA)".into()));
                    return;
                }

                // Send OTA_START with firmware size
                let size = firmware.len() as u32;
                let size_payload = size.to_le_bytes().to_vec();
                match ser.send_binary(logic::binary_protocol::cmd::OTA_START, &size_payload) {
                    Ok(_) => {}
                    Err(e) => {
                        let _ = tx.send(BgMsg::OtaProgress(0.0, format!("OTA start error: {}", e)));
                        return;
                    }
                }

                let _ = tx.send(BgMsg::OtaProgress(0.05, "OTA started, sending data...".into()));

                // Send data in 512-byte chunks
                let chunk_size = 512;
                let total_chunks = (firmware.len() + chunk_size - 1) / chunk_size;
                for (i, chunk) in firmware.chunks(chunk_size).enumerate() {
                    match ser.send_binary(logic::binary_protocol::cmd::OTA_DATA, chunk) {
                        Ok(_) => {}
                        Err(e) => {
                            let _ = ser.send_binary(logic::binary_protocol::cmd::OTA_ABORT, &[]);
                            let _ = tx.send(BgMsg::OtaProgress(0.0, format!("OTA data error at chunk {}: {}", i, e)));
                            return;
                        }
                    }
                    let progress = 0.05 + 0.90 * ((i + 1) as f32 / total_chunks as f32);
                    if (i + 1) % 20 == 0 || i + 1 == total_chunks {
                        let _ = tx.send(BgMsg::OtaProgress(
                            progress,
                            format!("Sending {}/{} ({} KB / {} KB)", i + 1, total_chunks, ((i + 1) * chunk_size).min(firmware.len()) / 1024, firmware.len() / 1024),
                        ));
                    }
                }

                let _ = tx.send(BgMsg::OtaProgress(1.0, "OTA complete — device will reboot".into()));
            });
        });
    }

    // --- SettingsBridge: flash select file ---
    {
        let flash_firmware_path = flash_firmware_path.clone();
        let window_weak = window.as_weak();
        window.global::<SettingsBridge>().on_flash_select_file(move || {
            let flash_firmware_path = flash_firmware_path.clone();
            let window_weak = window_weak.clone();
            std::thread::spawn(move || {
                let dialog = rfd::FileDialog::new()
                    .set_title("Select ESP32 Firmware")
                    .add_filter("Binary", &["bin"])
                    .pick_file();

                if let Some(path) = dialog {
                    let display = path.display().to_string();
                    *flash_firmware_path.lock().unwrap() = display.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = window_weak.upgrade() {
                            w.global::<SettingsBridge>().set_prog_path(SharedString::from(&display));
                        }
                    });
                }
            });
        });
    }

    // --- SettingsBridge: flash start ---
    {
        let flash_firmware_path = flash_firmware_path.clone();
        let tx = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<SettingsBridge>().on_flash_start(move || {
            let fw_path = flash_firmware_path.lock().unwrap().clone();
            if fw_path.is_empty() { return; }

            // Get selected port name from the bridge
            let port_name = if let Some(w) = window_weak.upgrade() {
                let bridge = w.global::<SettingsBridge>();
                let idx = bridge.get_selected_prog_port() as usize;
                let ports_model = bridge.get_prog_ports();
                if idx < ports_model.row_count() {
                    ports_model.row_data(idx).map(|s| s.to_string()).unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            if port_name.is_empty() {
                let _ = tx.send(BgMsg::FlashProgress(0.0, "No port selected".into()));
                return;
            }

            let tx = tx.clone();
            std::thread::spawn(move || {
                let firmware = match std::fs::read(&fw_path) {
                    Ok(data) => data,
                    Err(e) => {
                        let _ = tx.send(BgMsg::FlashProgress(0.0, format!("File read error: {}", e)));
                        return;
                    }
                };

                let (flash_tx, flash_rx) = mpsc::channel();
                let port_name_clone = port_name.clone();
                let flash_handle = std::thread::spawn(move || {
                    logic::flasher::flash_firmware(&port_name_clone, &firmware, 0x10000, &flash_tx)
                });

                // Forward progress messages
                while let Ok(logic::flasher::FlashProgress::OtaProgress(progress, msg)) = flash_rx.recv() {
                    let _ = tx.send(BgMsg::FlashProgress(progress, msg));
                }

                match flash_handle.join() {
                    Ok(Ok(())) => {
                        let _ = tx.send(BgMsg::FlashProgress(1.0, "Flash complete!".into()));
                    }
                    Ok(Err(e)) => {
                        let _ = tx.send(BgMsg::FlashProgress(0.0, format!("Flash error: {}", e)));
                    }
                    Err(_) => {
                        let _ = tx.send(BgMsg::FlashProgress(0.0, "Flash thread panicked".into()));
                    }
                }
            });
        });
    }

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

        // Tick counter: incremented every 200ms
        // Every 15 ticks (3s): auto-reconnect if disconnected
        // Every 10 ticks (2s): WPM poll if connected
        let tick_counter = std::cell::Cell::new(0u32);
        // Track if a reconnect attempt is in progress to avoid spamming
        let reconnect_in_progress = Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Notification auto-hide: counts down from 15 (3 seconds) when shown
        let notification_countdown = std::cell::Cell::new(0u32);

        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(200),
            move || {
                let Some(window) = window_weak.upgrade() else { return };

                // Increment tick counter
                let ticks = tick_counter.get().wrapping_add(1);
                tick_counter.set(ticks);

                // Notification auto-hide logic
                {
                    let app = window.global::<AppState>();
                    if app.get_notification_visible() {
                        let count = notification_countdown.get();
                        if count == 0 {
                            // Just became visible: start countdown (15 ticks = 3s)
                            notification_countdown.set(15);
                        } else if count == 1 {
                            // Time's up: hide notification
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
                            let mut ser = serial.lock().unwrap();
                            match ser.auto_connect() {
                                Ok(port_name) => {
                                    let fw = ser.get_firmware_version().unwrap_or_default();
                                    let names = ser.get_layer_names().unwrap_or_default();
                                    let km = ser.get_keymap(0).unwrap_or_default();
                                    let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));

                                    if let Ok(json) = ser.get_layout_json() {
                                        if let Ok(keys) = logic::layout::parse_json(&json) {
                                            let _ = tx.send(BgMsg::LayoutJson(keys));
                                        }
                                    }
                                }
                                Err(_) => {
                                    // Silently fail — will retry next cycle
                                }
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
                            let mut ser = serial.lock().unwrap();
                            if ser.connected {
                                match ser.get_wpm() {
                                    Ok(wpm) => { let _ = tx.send(BgMsg::Wpm(wpm)); }
                                    Err(_) => {} // Silently ignore WPM errors
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

                            // Update layer names
                            let new_layers = build_layer_model(&names);
                            window.global::<KeymapBridge>().set_layers(ModelRc::from(new_layers));

                            // Update keymap
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
                            // Replace the shared layout
                            *keys_arc.lock().unwrap() = new_keys.clone();

                            // Rebuild keycap data and repopulate existing model
                            // (keeps all Rc<VecModel> references valid)
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

                            // If there's a current keymap, update labels
                            let km = current_keymap.borrow();
                            if !km.is_empty() {
                                update_keycap_labels(&keycap_model, &new_keys, &km, &keyboard_layout.borrow());
                            }

                            // Update bounding box for responsive scaling
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
                                // Look up key labels from the current keymap instead of "rXcY"
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
                                // Build sequence string: "A -> B -> C"
                                let seq_str = l.sequence.iter()
                                    .map(|&k| keycode::hid_key_name(k))
                                    .collect::<Vec<_>>()
                                    .join(" -> ");
                                // Build result string: "Key" or "Key + Mod"
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
                            // Parse BT status lines
                            // First line: "BT: slot=X init=Y conn=Z pairing=W"
                            // Subsequent: "BT slot N: valid=V addr=AA:BB:CC:DD:EE:FF name=NAME"
                            let mut active_slot: i32 = 0;
                            let mut connected: u8 = 0;
                            let mut bt_mode = "USB";
                            let mut slots: Vec<BtSlotInfo> = Vec::new();

                            for line in &bt_lines {
                                if line.starts_with("BT:") {
                                    // Parse global state
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
                                    // Parse slot info
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
                            // Build heatmap keycaps: same positions as regular but
                            // colored by press frequency
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

                                    // Get key label from keymap
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

                            // Build stats summary using stats_analyzer
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

                            // If keymap heatmap toggle is active, also colorize the main keycap model
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
