use crate::msg::{AppShared, BgMsg};
use crate::models::update_keycap_labels;
use crate::logic::{binary_protocol, protocol};
use crate::{MainWindow, AppState, KeymapBridge};
use slint::{ComponentHandle, Model, SharedString};

pub fn setup(window: &MainWindow, shared: &AppShared) {
    setup_select_key(window, shared);
    setup_switch_layer(window, shared);
    setup_toggle_heatmap(window, shared);
    setup_export_keymap(window, shared);
    setup_import_keymap(window, shared);
    setup_rename_layer(window, shared);
}

fn setup_select_key(window: &MainWindow, shared: &AppShared) {
    let keycap_model = shared.keycap_model.clone();
    let window_weak = window.as_weak();
    window.global::<KeymapBridge>().on_select_key(move |key_index| {
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

fn setup_switch_layer(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let layer_model = shared.layer_model.clone();
    let current_layer = shared.current_layer.clone();
    let window_weak = window.as_weak();
    let window_weak_layer = window.as_weak();

    window.global::<KeymapBridge>().on_switch_layer(move |layer_index| {
        let idx = layer_index as usize;
        current_layer.set(idx);

        if let Some(w) = window_weak_layer.upgrade() {
            w.global::<KeymapBridge>().set_active_layer(layer_index);
        }

        for i in 0..layer_model.row_count() {
            let mut item = layer_model.row_data(i).unwrap();
            let should_be_active = item.index == layer_index;
            if item.active != should_be_active {
                item.active = should_be_active;
                layer_model.set_row_data(i, item);
            }
        }

        if let Some(w) = window_weak.upgrade() {
            w.global::<AppState>().set_status_text(SharedString::from(format!("Loading layer {}...", idx)));
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
            match ser.get_keymap(idx as u8) {
                Ok(km) => { tx.send(BgMsg::Keymap(km)); }
                Err(e) => { tx.send(BgMsg::ConnectError(e)); }
            }
        });
    });
}

fn setup_toggle_heatmap(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let keycap_model = shared.keycap_model.clone();
    let keys_arc = shared.keys.clone();
    let current_keymap = shared.current_keymap.clone();
    let keyboard_layout = shared.keyboard_layout.clone();
    let window_weak = window.as_weak();
    window.global::<KeymapBridge>().on_toggle_heatmap(move |enabled| {
        if enabled {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Loading heatmap...".into());
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
                        Err(e) => { tx.send(BgMsg::StatusMsg(format!("Heatmap error: {}", e))); }
                    }
                } else {
                    match ser.query_command(protocol::CMD_KEYSTATS) {
                        Ok(lines) => {
                            let (data, max_val) = crate::logic::parsers::parse_heatmap_lines(&lines);
                            tx.send(BgMsg::StatsData(data, max_val));
                        }
                        Err(e) => { tx.send(BgMsg::StatusMsg(format!("Heatmap error: {}", e))); }
                    }
                }
            });
        } else {
            let keys_guard = keys_arc.lock().unwrap();
            let km = current_keymap.borrow();
            for i in 0..keycap_model.row_count() {
                let mut item = keycap_model.row_data(i).unwrap();
                item.color = slint::Color::from_argb_u8(255, 0x44, 0x47, 0x5a);
                item.sublabel = SharedString::default();
                keycap_model.set_row_data(i, item);
            }
            if !km.is_empty() {
                update_keycap_labels(&keycap_model, &keys_guard, &km, &keyboard_layout.borrow());
            }
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("Heatmap disabled".into());
            }
        }
    });
}

fn setup_export_keymap(window: &MainWindow, shared: &AppShared) {
    let current_keymap = shared.current_keymap.clone();
    let current_layer = shared.current_layer.clone();
    let window_weak = window.as_weak();
    window.global::<KeymapBridge>().on_export_keymap(move || {
        let km = current_keymap.borrow();
        let layer = current_layer.get();
        if km.is_empty() {
            if let Some(w) = window_weak.upgrade() {
                w.global::<AppState>().set_status_text("No keymap data to export".into());
            }
            return;
        }

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

fn setup_import_keymap(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let current_layer = shared.current_layer.clone();
    window.global::<KeymapBridge>().on_import_keymap(move || {
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
                    tx.send(BgMsg::StatusMsg("Import cancelled".into()));
                    return;
                }
            };

            let contents = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("Read error: {}", e)));
                    return;
                }
            };

            let parsed: serde_json::Value = match serde_json::from_str(&contents) {
                Ok(v) => v,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("JSON parse error: {}", e)));
                    return;
                }
            };

            let target_layer = parsed.get("layer")
                .and_then(|v| v.as_u64())
                .map(|l| l as u8)
                .unwrap_or(layer);

            let rows = match parsed.get("rows").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => {
                    tx.send(BgMsg::StatusMsg("Invalid keymap JSON: missing 'rows'".into()));
                    return;
                }
            };

            let mut ser = match serial.lock() {
                Ok(s) => s,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("Lock error: {}", e)));
                    return;
                }
            };
            if !ser.connected {
                tx.send(BgMsg::StatusMsg("Not connected".into()));
                return;
            }

            tx.send(BgMsg::StatusMsg(format!("Importing keymap to layer {}...", target_layer)));

            for (row_idx, row_val) in rows.iter().enumerate() {
                let cols = match row_val.as_array() {
                    Some(c) => c,
                    None => continue,
                };
                for (col_idx, code_val) in cols.iter().enumerate() {
                    let code = code_val.as_u64().unwrap_or(0) as u16;
                    if let Err(e) = ser.set_key(target_layer, row_idx as u8, col_idx as u8, code) {
                        tx.send(BgMsg::StatusMsg(format!("Import error at R{}C{}: {}", row_idx, col_idx, e)));
                        return;
                    }
                }
            }

            match ser.get_keymap(target_layer) {
                Ok(km) => { tx.send(BgMsg::Keymap(km)); }
                Err(_) => {}
            }
            tx.send(BgMsg::Notification("Keymap imported successfully".into()));
        });
    });
}

fn setup_rename_layer(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<KeymapBridge>().on_rename_layer(move |layer_index, new_name| {
        let name = new_name.to_string();
        if name.is_empty() {
            tx.send(BgMsg::StatusMsg("Layer name cannot be empty".into()));
            return;
        }
        let serial = serial.clone();
        let tx = tx.clone();
        let layer = layer_index as u8;
        std::thread::spawn(move || {
            let mut ser = match serial.lock() {
                Ok(s) => s,
                Err(e) => {
                    tx.send(BgMsg::StatusMsg(format!("Lock error: {}", e)));
                    return;
                }
            };
            if !ser.connected {
                tx.send(BgMsg::StatusMsg("Not connected".into()));
                return;
            }

            if ser.v2 {
                let mut payload = vec![layer];
                payload.extend_from_slice(name.as_bytes());
                match ser.send_binary(binary_protocol::cmd::LAYER_NAME, &payload) {
                    Ok(_) => {
                        tx.send(BgMsg::Notification(format!("Layer {} renamed to '{}'", layer, name)));
                    }
                    Err(e) => {
                        tx.send(BgMsg::StatusMsg(format!("Rename error: {}", e)));
                        return;
                    }
                }
            } else {
                let cmd = protocol::cmd_set_layer_name(layer, &name);
                if let Err(e) = ser.send_command(&cmd) {
                    tx.send(BgMsg::StatusMsg(format!("Rename error: {}", e)));
                    return;
                }
                tx.send(BgMsg::Notification(format!("Layer {} renamed to '{}'", layer, name)));
            }

            match ser.get_layer_names() {
                Ok(names) => { tx.send(BgMsg::LayerNames(names)); }
                Err(_) => {}
            }
        });
    });
}
