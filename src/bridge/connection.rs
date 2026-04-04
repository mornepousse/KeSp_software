use crate::msg::{AppShared, BgMsg};
use crate::logic::serial::SerialManager;
use crate::{MainWindow, AppState, ConnectionBridge, ConnectionState, PortInfo};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::rc::Rc;

pub fn setup(window: &MainWindow, shared: &AppShared) {
    setup_initial_ports(window);
    setup_auto_connect(window, shared);
    setup_connect(window, shared);
    setup_disconnect(window, shared);
    setup_refresh_ports(window, shared);
}

fn setup_initial_ports(window: &MainWindow) {
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

fn setup_auto_connect(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<AppState>().set_status_text("Scanning ports...".into());
    window.global::<AppState>().set_connection(ConnectionState::Connecting);

    std::thread::spawn(move || {
        let mut ser = match serial.lock() {
            Ok(s) => s,
            Err(e) => {
                tx.send(BgMsg::ConnectError(format!("Lock error: {}", e)));
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
                    if let Ok(keys) = crate::logic::layout::parse_json(&json) {
                        tx.send(BgMsg::LayoutJson(keys));
                    }
                }
            }
            Err(e) => {
                tx.send(BgMsg::ConnectError(e));
            }
        }
    });
}

fn setup_connect(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    let window_weak = window.as_weak();
    window.global::<ConnectionBridge>().on_connect(move || {
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
        let serial = serial.clone();
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut ser = match serial.lock() {
                Ok(s) => s,
                Err(e) => {
                    tx.send(BgMsg::ConnectError(format!("Lock error: {}", e)));
                    return;
                }
            };
            let connect_result = if selected_path.is_empty() {
                ser.auto_connect()
            } else {
                ser.connect(&selected_path).map(|_| selected_path.clone())
            };
            match connect_result {
                Ok(port_name) => {
                    let fw = ser.get_firmware_version().unwrap_or_default();
                    let names = ser.get_layer_names().unwrap_or_default();
                    let km = ser.get_keymap(0).unwrap_or_default();
                    tx.send(BgMsg::Connected(port_name, fw, names, km));

                    if let Ok(json) = ser.get_layout_json() {
                        if let Ok(keys) = crate::logic::layout::parse_json(&json) {
                            tx.send(BgMsg::LayoutJson(keys));
                        }
                    }
                }
                Err(e) => { tx.send(BgMsg::ConnectError(e)); }
            }
        });
    });
}

fn setup_disconnect(window: &MainWindow, shared: &AppShared) {
    let serial = shared.serial.clone();
    let tx = shared.tx.clone();
    window.global::<ConnectionBridge>().on_disconnect(move || {
        match serial.lock() {
            Ok(mut ser) => {
                ser.disconnect();
                tx.send(BgMsg::Disconnected);
            }
            Err(e) => {
                tx.send(BgMsg::StatusMsg(format!("Lock error: {}", e)));
            }
        }
    });
}

fn setup_refresh_ports(window: &MainWindow, shared: &AppShared) {
    let tx = shared.tx.clone();
    window.global::<ConnectionBridge>().on_refresh_ports(move || {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let ports = SerialManager::list_ports_detailed();
            tx.send(BgMsg::PortList(ports));
        });
    });
}
