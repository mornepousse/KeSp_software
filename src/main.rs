mod logic;

slint::include_modules!();

use logic::keycode;
use logic::layout::KeycapPos;
use logic::serial::SerialManager;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::rc::Rc;

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
    ConnectError(String),
    Keymap(Vec<Vec<u16>>),
    LayerNames(Vec<String>),
    Disconnected,
    TapDanceData(Vec<[u16; 4]>),
    ComboData(Vec<logic::parsers::ComboEntry>),
    StatusMsg(String),
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

fn main() {
    let keys = logic::layout::default_layout();
    let num_keys = keys.len();
    let keys_arc = Arc::new(keys.clone());

    let keycap_model = build_keycap_model(&keys);
    let layer_model = build_layer_model(&["Layer 0".into(), "Layer 1".into(), "Layer 2".into(), "Layer 3".into()]);

    let window = MainWindow::new().unwrap();

    // Set up models
    let keymap_bridge = window.global::<KeymapBridge>();
    keymap_bridge.set_keycaps(ModelRc::from(keycap_model.clone()));
    keymap_bridge.set_layers(ModelRc::from(layer_model.clone()));

    // Serial manager shared between threads
    let serial: Arc<Mutex<SerialManager>> = Arc::new(Mutex::new(SerialManager::new()));
    let (bg_tx, bg_rx) = mpsc::channel::<BgMsg>();

    // Current state
    let current_keymap: Rc<std::cell::RefCell<Vec<Vec<u16>>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
    let current_layer: Rc<std::cell::Cell<usize>> = Rc::new(std::cell::Cell::new(0));
    let keyboard_layout = Rc::new(logic::layout_remap::KeyboardLayout::from_name("QWERTY"));

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
            if idx >= keys_arc.len() { return; }

            let kp = &keys_arc[idx];
            let row = kp.row as usize;
            let col = kp.col as usize;

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
                let remapped = logic::layout_remap::remap_key_label(&keyboard_layout, &decoded);
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
            if idx >= num_keys { return; }
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

        keymap_bridge.on_switch_layer(move |layer_index| {
            let idx = layer_index as usize;
            current_layer.set(idx);

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

    // --- Connect/Disconnect callbacks ---
    {
        let serial_c = serial.clone();
        let tx_c = bg_tx.clone();
        let window_weak = window.as_weak();
        window.global::<ConnectionBridge>().on_connect(move || {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Scanning ports...".into());
                w.global::<AppState>().set_connection(ConnectionState::Connecting);
            }
            let serial = serial_c.clone();
            let tx = tx_c.clone();
            std::thread::spawn(move || {
                let mut ser = serial.lock().unwrap();
                match ser.auto_connect() {
                    Ok(port_name) => {
                        let fw = ser.get_firmware_version().unwrap_or_default();
                        let names = ser.get_layer_names().unwrap_or_default();
                        let km = ser.get_keymap(0).unwrap_or_default();
                        let _ = tx.send(BgMsg::Connected(port_name, fw, names, km));
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

    window.global::<ConnectionBridge>().on_refresh_ports(|| {});

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

    // --- Poll background messages via timer ---
    {
        let window_weak = window.as_weak();
        let keycap_model = keycap_model.clone();
        let keys_arc = keys_arc.clone();
        let current_keymap = current_keymap.clone();
        let keyboard_layout = keyboard_layout.clone();

        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(50),
            move || {
                let Some(window) = window_weak.upgrade() else { return };

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
                            update_keycap_labels(&keycap_model, &keys_arc, &km, &keyboard_layout);
                        }
                        BgMsg::ConnectError(e) => {
                            let app = window.global::<AppState>();
                            app.set_connection(ConnectionState::Disconnected);
                            app.set_status_text(SharedString::from(format!("Error: {}", e)));
                        }
                        BgMsg::Keymap(km) => {
                            *current_keymap.borrow_mut() = km.clone();
                            update_keycap_labels(&keycap_model, &keys_arc, &km, &keyboard_layout);
                            window.global::<AppState>().set_status_text("Keymap loaded".into());
                        }
                        BgMsg::LayerNames(names) => {
                            let new_layers = build_layer_model(&names);
                            window.global::<KeymapBridge>().set_layers(ModelRc::from(new_layers));
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
                            let model: Vec<ComboEntry> = combos.iter().map(|c| {
                                ComboEntry {
                                    index: c.index as i32,
                                    key1_label: SharedString::from(format!("r{}c{}", c.r1, c.c1)),
                                    key2_label: SharedString::from(format!("r{}c{}", c.r2, c.c2)),
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
                        BgMsg::StatusMsg(msg) => {
                            window.global::<AppState>().set_status_text(SharedString::from(msg));
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
