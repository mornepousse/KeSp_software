mod logic;

slint::include_modules!();

use logic::keycode;
use logic::layout::KeycapPos;
use logic::serial::SerialManager;
use slint::{Model, ModelRc, SharedString, VecModel};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::rc::Rc;

// Messages from background serial thread to UI
enum BgMsg {
    Connected(String, String, Vec<String>, Vec<Vec<u16>>), // port, fw_version, layer_names, keymap
    ConnectError(String),
    Keymap(Vec<Vec<u16>>),
    LayerNames(Vec<String>),
    Disconnected,
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
                    }
                }
            },
        );

        // Keep timer alive
        let _keep_timer = timer;
        window.run().unwrap();
    }
}
