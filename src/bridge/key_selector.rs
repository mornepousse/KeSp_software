use crate::msg::AppShared;
use crate::keycode_catalog::{build_keycode_entries, filter_keycode_entries};
use crate::logic::{keycode, layout_remap};
use crate::{MainWindow, KeymapBridge, KeySelectorBridge, KeycodeEntry, ModalKind};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

pub fn setup(window: &MainWindow, shared: &AppShared) {
    let all_keycode_entries = build_keycode_entries();
    let keycode_model: Rc<VecModel<KeycodeEntry>> =
        Rc::new(VecModel::from(all_keycode_entries.clone()));

    window.global::<KeySelectorBridge>().set_entries(ModelRc::from(keycode_model.clone()));

    setup_search(window, &all_keycode_entries, &keycode_model);
    setup_select_keycode(window, shared, &all_keycode_entries, &keycode_model);
    setup_cancel(window, &all_keycode_entries, &keycode_model);
    setup_apply_mt(window);
    setup_apply_lt(window);
    setup_apply_hex(window);
}

fn setup_search(
    window: &MainWindow,
    all_entries: &[KeycodeEntry],
    keycode_model: &Rc<VecModel<KeycodeEntry>>,
) {
    let all_entries = all_entries.to_vec();
    let keycode_model = keycode_model.clone();
    window.global::<KeySelectorBridge>().on_search_changed(move |text| {
        let filtered = filter_keycode_entries(&all_entries, text.as_str());
        let count = keycode_model.row_count();
        for _ in 0..count {
            keycode_model.remove(0);
        }
        for e in filtered {
            keycode_model.push(e);
        }
    });
}

fn setup_select_keycode(
    window: &MainWindow,
    shared: &AppShared,
    all_entries: &[KeycodeEntry],
    keycode_model: &Rc<VecModel<KeycodeEntry>>,
) {
    let keycap_model = shared.keycap_model.clone();
    let keys_arc = shared.keys.clone();
    let current_keymap = shared.current_keymap.clone();
    let current_layer = shared.current_layer.clone();
    let keyboard_layout = shared.keyboard_layout.clone();
    let serial = shared.serial.clone();
    let all_entries = all_entries.to_vec();
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
            let remapped = layout_remap::remap_key_label(&layout, &decoded);
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
            if let Ok(mut ser) = serial.lock() {
                if let Err(e) = ser.set_key(layer, r, c, keycode_val) {
                    eprintln!("Failed to send key change: {}", e);
                }
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

fn setup_cancel(
    window: &MainWindow,
    all_entries: &[KeycodeEntry],
    keycode_model: &Rc<VecModel<KeycodeEntry>>,
) {
    let window_weak = window.as_weak();
    let all_entries = all_entries.to_vec();
    let keycode_model = keycode_model.clone();
    window.global::<KeySelectorBridge>().on_cancel(move || {
        if let Some(w) = window_weak.upgrade() {
            let ks = w.global::<KeySelectorBridge>();
            ks.set_active_modal(ModalKind::None);
            ks.set_search_filter(SharedString::default());
        }
        let count = keycode_model.row_count();
        for _ in 0..count {
            keycode_model.remove(0);
        }
        for e in &all_entries {
            keycode_model.push(e.clone());
        }
    });
}

fn setup_apply_mt(window: &MainWindow) {
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

fn setup_apply_lt(window: &MainWindow) {
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

fn setup_apply_hex(window: &MainWindow) {
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
