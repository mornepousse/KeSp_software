use crate::logic;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::rc::Rc;
use std::cell::{Cell, RefCell};

use crate::logic::layout::KeycapPos;
use crate::logic::layout_remap::KeyboardLayout;
use crate::logic::serial::SerialManager;
use crate::{KeycapData, LayerInfo};
use slint::VecModel;

/// Sender wrapper that wakes the Slint event loop after each send.
/// This eliminates the need for a polling timer.
#[derive(Clone)]
pub struct UiSender {
    tx: mpsc::Sender<BgMsg>,
    /// Sending a no-op invoke_from_event_loop wakes the event loop,
    /// which will then process the pending message via a zero-delay timer.
    _wake: Arc<dyn Fn() + Send + Sync>,
}

impl UiSender {
    pub fn new(tx: mpsc::Sender<BgMsg>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { tx, _wake: wake }
    }
    pub fn send(&self, msg: BgMsg) {
        let _ = self.tx.send(msg);
        (self._wake)();
    }
}

/// Messages from background serial thread to UI
pub enum BgMsg {
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

/// Shared application state passed to all bridge setup functions.
pub struct AppShared {
    pub serial: Arc<Mutex<SerialManager>>,
    pub tx: UiSender,
    pub keys: Arc<Mutex<Vec<KeycapPos>>>,
    pub current_keymap: Rc<RefCell<Vec<Vec<u16>>>>,
    pub keyboard_layout: Rc<RefCell<KeyboardLayout>>,
    pub keycap_model: Rc<VecModel<KeycapData>>,
    pub layer_model: Rc<VecModel<LayerInfo>>,
    pub current_layer: Rc<Cell<usize>>,
    pub macro_steps: Rc<RefCell<Vec<(String, u8, u32)>>>,
    pub macro_entries: Rc<RefCell<Vec<logic::parsers::MacroEntry>>>,
    pub ota_firmware_path: Arc<Mutex<String>>,
    pub flash_firmware_path: Arc<Mutex<String>>,
}

/// Spawn a simple serial command (binary v2 + legacy fallback).
/// Reduces boilerplate for commands that just send and report status.
pub fn spawn_command(
    serial: &Arc<Mutex<SerialManager>>,
    tx: &UiSender,
    v2_cmd: u8,
    legacy_cmd: &str,
    success_msg: &str,
) {
    let serial = serial.clone();
    let tx = tx.clone();
    let success_msg = success_msg.to_string();
    let legacy_cmd = legacy_cmd.to_string();
    std::thread::spawn(move || {
        let mut ser = match serial.lock() {
            Ok(s) => s,
            Err(e) => {
                tx.send(BgMsg::StatusMsg(format!("Lock error: {}", e)));
                return;
            }
        };
        if ser.v2 {
            match ser.send_binary(v2_cmd, &[]) {
                Ok(_) => { tx.send(BgMsg::StatusMsg(success_msg)); }
                Err(e) => { tx.send(BgMsg::StatusMsg(format!("Error: {}", e))); }
            }
        } else {
            let _ = ser.send_command(&legacy_cmd);
            tx.send(BgMsg::StatusMsg(success_msg));
        }
    });
}
