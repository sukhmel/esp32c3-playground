//! Parts taken from <https://github.com/bjoernQ/esp32c3-ble-hid>, parts from <https://github.com/embassy-rs/trouble/blob/trouble-host-v0.5.1/examples/apps/src/ble_bas_peripheral.rs>

use crate::inter_task::{Keypress, KeypressReceiver, BLE_CONNECTED};
use ariel_os::ble::ble_stack;
use ariel_os::debug::log::{info, warn};
use ariel_os::time::{Duration, Timer};
use bt_hci::param::Status;
use bt_hci::cmd::le::{LeConnUpdate, LeReadLocalSupportedFeatures};
use bt_hci::controller::{ControllerCmdAsync, ControllerCmdSync};
use embassy_futures::join::join;
use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use trouble_host::Stack;
use trouble_host::prelude::*;

macro_rules! count {
	() => { 0u8 };
	($x:tt $($xs:tt)*) => {1u8 + count!($($xs)*)};
}

macro_rules! hid {
	($(( $($xs:tt),*)),+ $(,)?) => { [ $( (count!($($xs)*)-1) | $($xs),* ),* ] };
}

// Main items
pub const HIDINPUT: u8 = 0x80;
pub const HIDOUTPUT: u8 = 0x90;
pub const FEATURE: u8 = 0xb0;
pub const COLLECTION: u8 = 0xa0;
pub const END_COLLECTION: u8 = 0xc0;

// Global items
pub const USAGE_PAGE: u8 = 0x04;
pub const LOGICAL_MINIMUM: u8 = 0x14;
pub const LOGICAL_MAXIMUM: u8 = 0x24;
pub const PHYSICAL_MINIMUM: u8 = 0x34;
pub const PHYSICAL_MAXIMUM: u8 = 0x44;
pub const UNIT_EXPONENT: u8 = 0x54;
pub const UNIT: u8 = 0x64;
pub const REPORT_SIZE: u8 = 0x74; //bits
pub const REPORT_ID: u8 = 0x84;
pub const REPORT_COUNT: u8 = 0x94; //bytes
pub const PUSH: u8 = 0xa4;
pub const POP: u8 = 0xb4;

// Local items
pub const USAGE: u8 = 0x08;
pub const USAGE_MINIMUM: u8 = 0x18;
pub const USAGE_MAXIMUM: u8 = 0x28;
pub const DESIGNATOR_INDEX: u8 = 0x38;
pub const DESIGNATOR_MINIMUM: u8 = 0x48;
pub const DESIGNATOR_MAXIMUM: u8 = 0x58;
pub const STRING_INDEX: u8 = 0x78;
pub const STRING_MINIMUM: u8 = 0x88;
pub const STRING_MAXIMUM: u8 = 0x98;
pub const DELIMITER: u8 = 0xa8;

const KEYBOARD_ID: u8 = 0x01;

const HID_REPORT_MAP: [u8; 65] = hid!(
    (USAGE_PAGE, 0x01), // USAGE_PAGE (Generic Desktop Ctrls)
    (USAGE, 0x06),      // USAGE (Keyboard)
    (COLLECTION, 0x01), // COLLECTION (Application)
    // ------------------------------------------------- Keyboard
    (REPORT_ID, KEYBOARD_ID), //   REPORT_ID (1)
    (USAGE_PAGE, 0x07),       //   USAGE_PAGE (Kbrd/Keypad)
    (USAGE_MINIMUM, 0xE0),    //   USAGE_MINIMUM (0xE0)
    (USAGE_MAXIMUM, 0xE7),    //   USAGE_MAXIMUM (0xE7)
    (LOGICAL_MINIMUM, 0x00),  //   LOGICAL_MINIMUM (0)
    (LOGICAL_MAXIMUM, 0x01),  //   Logical Maximum (1)
    (REPORT_SIZE, 0x01),      //   REPORT_SIZE (1)
    (REPORT_COUNT, 0x08),     //   REPORT_COUNT (8)
    (HIDINPUT, 0x02), //   INPUT (Data,Var,Abs,No Wrap,Linear,Preferred State,No Null Position)
    (REPORT_COUNT, 0x01), //   REPORT_COUNT (1) ; 1 byte (Reserved)
    (REPORT_SIZE, 0x08), //   REPORT_SIZE (8)
    (HIDINPUT, 0x01), //   INPUT (Const,Array,Abs,No Wrap,Linear,Preferred State,No Null Position)
    (REPORT_COUNT, 0x05), //   REPORT_COUNT (5) ; 5 bits (Num lock, Caps lock, Scroll lock, Compose, Kana)
    (REPORT_SIZE, 0x01),  //   REPORT_SIZE (1)
    (USAGE_PAGE, 0x08),   //   USAGE_PAGE (LEDs)
    (USAGE_MINIMUM, 0x01), //   USAGE_MINIMUM (0x01) ; Num Lock
    (USAGE_MAXIMUM, 0x05), //   USAGE_MAXIMUM (0x05) ; Kana
    (HIDOUTPUT, 0x02), //   OUTPUT (Data,Var,Abs,No Wrap,Linear,Preferred State,No Null Position,Non-volatile)
    (REPORT_COUNT, 0x01), //   REPORT_COUNT (1) ; 3 bits (Padding)
    (REPORT_SIZE, 0x03), //   REPORT_SIZE (3)
    (HIDOUTPUT, 0x01), //   OUTPUT (Const,Array,Abs,No Wrap,Linear,Preferred State,No Null Position,Non-volatile)
    (REPORT_COUNT, 0x06), //   REPORT_COUNT (6) ; 6 bytes (Keys)
    (REPORT_SIZE, 0x08), //   REPORT_SIZE(8)
    (LOGICAL_MINIMUM, 0x00), //   LOGICAL_MINIMUM(0)
    (LOGICAL_MAXIMUM, 0x65), //   LOGICAL_MAXIMUM(0x65) ; 101 keys
    (USAGE_PAGE, 0x07), //   USAGE_PAGE (Kbrd/Keypad)
    (USAGE_MINIMUM, 0x00), //   USAGE_MINIMUM (0)
    (USAGE_MAXIMUM, 0x65), //   USAGE_MAXIMUM (0x65)
    (HIDINPUT, 0x00),  //   INPUT (Data,Array,Abs,No Wrap,Linear,Preferred State,No Null Position)
    (END_COLLECTION),  // END_COLLECTION
);

struct KeyboardReport {
    modifiers: u8,
    reserved: u8,
    key_codes: [u8; 6],
}

impl KeyboardReport {
    fn to_bytes(&self) -> [u8; 8] {
        [
            self.modifiers,
            self.reserved,
            self.key_codes[0],
            self.key_codes[1],
            self.key_codes[2],
            self.key_codes[3],
            self.key_codes[4],
            self.key_codes[5],
        ]
    }
}

#[gatt_server]
struct Server {
    hid_service: HidService,
}

#[gatt_service(uuid = "1812")] // Standard Bluetooth HID Service UUID
struct HidService {
    // HID Information Characteristic (Country Code = 0, Flags = 0x01)
    #[characteristic(uuid = characteristic::HID_INFORMATION, read, value = [0x00, 0x01, 0x00, 0x01])]
    hid_info: [u8; 4],

    // HID Report Map Descriptor
    #[characteristic(uuid = characteristic::REPORT_MAP, read, value = HID_REPORT_MAP)]
    report_map: [u8; 65],

    // Protocol Mode (0 = Boot, 1 = Report). Hosts read/write this to select the
    // report protocol; without it many hosts refuse to enumerate the keyboard.
    #[characteristic(uuid = "2A4E", read, write_without_response, value = 0x01)]
    protocol_mode: u8,

    // Dynamic Input Report where key modifier/scan bytes are dispatched.
    // The Report Reference descriptor (0x2908) is REQUIRED: it tells the host
    // that this characteristic carries Input report ID 1. Without it the host
    // cannot map the report map's `REPORT_ID (1)` to this characteristic, so it
    // never builds the HID keyboard (no "device connected" badge, no keystrokes).
    #[characteristic(uuid = characteristic::REPORT, read, notify)]
    #[descriptor(uuid = "2908", read, value = [KEYBOARD_ID, 0x01])] // (Report ID, Type = Input)
    input_report: [u8; 8],

    // Required HID Control Point Handshake characteristic
    #[characteristic(uuid = characteristic::HID_CONTROL_POINT, write_without_response)]
    control_point: u8,
}

/// Connection parameters as confirmed by the central.
#[derive(Copy, Clone)]
struct NegotiatedParams {
    interval_ms: u32,
    latency: u16,
    timeout_ms: u32,
}

/// Fired by `gatt_events_task` whenever the link layer completes a connection
/// parameter update. `request_conn_params` waits on it to learn whether a
/// request was accepted — trouble-host reports a *rejection* only via an
/// internal warning (no event), so the absence of this signal within a timeout
/// is how we detect that the central refused.
static CONN_PARAMS_ACK: Signal<CriticalSectionRawMutex, NegotiatedParams> = Signal::new();

pub async fn serve_keyboard(mut channel: KeypressReceiver) -> ! {
    info!("keyboard: task started");
    let stack = ble_stack().await;
    let mut host = stack.build();
    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: "ESP32-C3 Joy KB",
        appearance: &appearance::human_interface_device::KEYBOARD,
    }))
        .unwrap();

    let ble_state = BLE_CONNECTED.sender();

    join(ble_task(host.runner), async {
        loop {
            match advertise("ESP32-C3 Joystick Keyboard", &mut host.peripheral, &server).await {
                Ok(conn) => {
                    info!("[connection] established!");
                    ble_state.send(true);

                    // Mark the link bondable BEFORE the central starts pairing.
                    // Without this the SMP negotiates "NoBonding", so: the peer
                    // never distributes its IRK (can't resolve its rotating
                    // random address later) and, crucially, trouble drops the
                    // bond on disconnect (it only keeps `is_bonded` bonds). The
                    // result is the "no long term key" / Authentication Failure
                    // loop after the peer reconnects. Bonding keeps the LTK +
                    // IRK in RAM for this power cycle (Ariel OS has no bond
                    // persistence yet, so it is lost on reboot — re-pair then).
                    if let Err(e) = conn.raw().set_bondable(true) {
                        warn!("[connection] could not set bondable: {:?}", e);
                    }

                    // Drop keypresses queued while nobody was connected, to not replay stale ones.
                    while channel.try_receive().is_ok() {}

                    // Run the connection loops until the link drops.
                    select(
                        gatt_events_task(&server, &conn),
                        join(
                            custom_task(&server, &conn, &mut channel),
                            request_conn_params(&conn, &*stack),
                        ),
                    ).await;

                    // Connection dropped: release the radio back to Wi-Fi.
                    ble_state.send(false);
                    info!("[connection] loop exited, ready to advertise again");
                    Timer::after(Duration::from_millis(200)).await;
                }
                Err(e) => {
                    warn!("[adv] error: {:?}", e);
                    Timer::after(Duration::from_secs(2)).await;
                }
            }
        }
    })
        .await
        .0
}

/// Ask the central for a relaxed connection interval to leave more time for a display task.
///
/// Runs once per connection, after a short delay to let pairing/encryption finish.
async fn request_conn_params<C, P: PacketPool>(
    conn: &GattConnection<'_, '_, P>,
    stack: &Stack<'_, C, P>,
) where
    C: ControllerCmdAsync<LeConnUpdate> + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    // Delay so pairing/encryption settles first; centrals commonly reject
    // parameter requests sent too early.
    Timer::after(Duration::from_secs(2)).await;

    // Candidates ordered from most-relaxed to least. We stop at the first one the central
    // accepts, so we always end up with the largest interval it will allow.
    const CANDIDATES: [(u64, u64, u16); 4] = [
        (50, 100, 9),
        (40, 75, 9),
        (30, 50, 9),
        (20, 40, 9),
    ];

    for (attempt, &(min_ms, max_ms, latency)) in CANDIDATES.iter().enumerate() {
        // Clear any stale/previous confirmation before issuing this request.
        CONN_PARAMS_ACK.reset();

        let params = ConnectParams {
            min_connection_interval: Duration::from_millis(min_ms),
            max_connection_interval: Duration::from_millis(max_ms),
            max_latency: latency,
            ..Default::default()
        };
        info!(
            "[conn] requesting interval {}-{} ms, latency {} (attempt {}/{})",
            min_ms,
            max_ms,
            latency,
            attempt + 1,
            CANDIDATES.len()
        );

        // `update_connection_params` returns once the command is accepted
        // locally; the actual accept/reject from the central arrives later as an
        // LL update-complete event. Success surfaces via CONN_PARAMS_ACK; a
        // rejection surfaces as nothing at all, so treat a timeout as failure.
        if conn.raw().update_connection_params(stack, &params).await.is_err() {
            warn!("[conn] controller rejected the request; trying a smaller interval");
            continue;
        }

        match select(Timer::after(Duration::from_secs(3)), CONN_PARAMS_ACK.wait()).await {
            Either::Second(p) => {
                info!(
                    "[conn] central accepted: interval {} ms, latency {}, timeout {} ms",
                    p.interval_ms, p.latency, p.timeout_ms
                );
                return;
            }
            Either::First(()) => {
                warn!("[conn] no confirmation (central refused?); trying a smaller interval");
            }
        }
    }

    warn!("[conn] could not negotiate a relaxed connection interval; keeping the central's default");
}

async fn custom_task<P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
    channel: &mut KeypressReceiver,
) {
    let mut report = [0u8; 8];

    loop {
        let keypress = channel.receive().await;

        match keypress {
            Keypress::Pressed(ch) => {
                if let Some(keystroke) = char_to_hid(ch) {
                    report[0] = keystroke.modifier;
                    report[2] = keystroke.keycode;
                }
            }
            Keypress::Released(_ch) => {
                report = [0u8; 8]; // Clear report on release
            }
        }

        if server
            .hid_service
            .input_report
            .notify(conn, &report)
            .await
            .is_err()
        {
            info!("[custom_task] error notifying connection or client not subscribed yet");
            Timer::after(Duration::from_millis(50)).await;
        }
    }
}

/// Stream Events until the connection closes.
async fn gatt_events_task(server: &Server<'_>, conn: &GattConnection<'_, '_, DefaultPacketPool>) -> Result<Option<Status>, Error> {
    loop {
        let event = conn.next().await;
        match event {
            GattConnectionEvent::Disconnected { reason } => return Ok(Some(reason)),
            GattConnectionEvent::PairingComplete { security_level, bond } => {
                match bond {
                    // `is_bonded` true means the bond survives disconnect (kept
                    // in RAM this power cycle); an IRK means the peer's rotating
                    // random address can be resolved on reconnect.
                    Some(b) => info!(
                        "[gatt] pairing complete: {:?}, bonded={}, irk={}",
                        security_level,
                        b.is_bonded,
                        b.identity.irk.is_some()
                    ),
                    None => info!(
                        "[gatt] pairing complete: {:?}, no bond stored",
                        security_level
                    ),
                }
            }
            GattConnectionEvent::ConnectionParamsUpdated {
                conn_interval,
                peripheral_latency,
                supervision_timeout,
            } => {
                let params = NegotiatedParams {
                    interval_ms: conn_interval.as_millis() as u32,
                    latency: peripheral_latency,
                    timeout_ms: supervision_timeout.as_millis() as u32,
                };
                info!(
                    "[conn] parameters now in effect: interval {} ms, latency {}, timeout {} ms",
                    params.interval_ms, params.latency, params.timeout_ms
                );
                CONN_PARAMS_ACK.signal(params);
            }
            GattConnectionEvent::PairingFailed(err) => {
                warn!("[gatt] pairing error: {:?}", err);
            }
            GattConnectionEvent::PassKeyInput => {
                info!("[gatt] passkey input");
                conn.pass_key_input(1234)?;
            }
            GattConnectionEvent::Gatt { event } => {
                match event {
                    GattEvent::Read(e) => {
                        info!("[gatt] handling ReadEvent");
                        match e.accept() {
                            Ok(reply) => { reply.send().await; }
                            Err(err) => warn!("[gatt] error creating read reply: {:?}", err),
                        }
                    }
                    GattEvent::Write(e) => {
                        info!("[gatt] handling WriteEvent");
                        match e.accept() {
                            Ok(reply) => { reply.send().await; }
                            Err(err) => warn!("[gatt] error creating write reply: {:?}", err),
                        }
                    }
                    other_gatt_event => {
                        // Catch-all for NotAllowed or structural events
                        info!("[gatt] handling other structural event");
                        match other_gatt_event.accept() {
                            Ok(reply) => { reply.send().await; }
                            Err(err) => warn!("[gatt] error accepting structural event: {:?}", err),
                        }
                    }
                }
            }
            _ => {}
        };
    }
}

/// Background task required to run forever alongside any other BLE tasks.
async fn ble_task<C: Controller, P: PacketPool>(mut runner: Runner<'_, C, P>) -> ! {
    loop {
        if let Err(e) = runner.run().await {
            panic!("[ble_task] error: {:?}", e);
        }
    }
}

/// Create an advertiser to use to connect to a BLE Central, and wait for it to connect.
async fn advertise<'values, 'server, C: Controller>(
    name: &'values str,
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut advertiser_data = [0; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            // Announces the HID service availability (0x1812)
            AdStructure::ServiceUuids16(&[[0x12, 0x18]]),
        ],
        &mut advertiser_data[..],
    )?;
    let mut scan_data = [0u8; 31];
    let mut scan_len = AdStructure::encode_slice(
        &[
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut scan_data[..],
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &advertiser_data[..len],
                scan_data: &scan_data[..scan_len],
            },
        )
        .await?;
    info!("[adv] advertising");
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    info!("[adv] connection established");
    Ok(conn)
}

#[derive(Copy, Clone, Debug)]
pub struct KeyStroke {
    pub modifier: u8,
    pub keycode: u8,
}

// Standard HID Modifier bits
const MODIFIER_NONE: u8 = 0x00;
const MODIFIER_SHIFT: u8 = 0x02; // Left Shift bit flag

// Index matches ASCII value directly. Format: (Modifier, KeyCode)
const ASCII_TO_HID: [(u8, u8); 128] = {
    let mut table = [(0u8, 0u8); 128];

    // Numbers 1-9, then 0
    table[b'1' as usize] = (MODIFIER_NONE, 0x1E);
    table[b'2' as usize] = (MODIFIER_NONE, 0x1F);
    table[b'3' as usize] = (MODIFIER_NONE, 0x20);
    table[b'4' as usize] = (MODIFIER_NONE, 0x21);
    table[b'5' as usize] = (MODIFIER_NONE, 0x22);
    table[b'6' as usize] = (MODIFIER_NONE, 0x23);
    table[b'7' as usize] = (MODIFIER_NONE, 0x24);
    table[b'8' as usize] = (MODIFIER_NONE, 0x25);
    table[b'9' as usize] = (MODIFIER_NONE, 0x26);
    table[b'0' as usize] = (MODIFIER_NONE, 0x27);

    // Special Controls
    table[b'\n' as usize] = (MODIFIER_NONE, 0x28); // Enter
    table[b'\t' as usize] = (MODIFIER_NONE, 0x2B); // Tab
    table[b' ' as usize] = (MODIFIER_NONE, 0x2C); // Spacebar

    // Populate letters 'a' through 'z' (HID codes 0x04 to 0x1D)
    let mut i = 0;
    while i < 26 {
        let lower_ascii = (b'a' + i) as usize;
        let upper_ascii = (b'A' + i) as usize;
        let hid_code = 0x04 + i;

        table[upper_ascii] = (MODIFIER_NONE, hid_code);
        table[lower_ascii] = (MODIFIER_SHIFT, hid_code); // Uppercase needs Shift, but it's swapped
        i += 1;
    }

    table[b',' as usize] = (MODIFIER_NONE, 0x36); // ,
    table[b'<' as usize] = (MODIFIER_SHIFT, 0x36); // <
    table[b'.' as usize] = (MODIFIER_NONE, 0x37); // .
    table[b'>' as usize] = (MODIFIER_SHIFT, 0x37); // >
    table[b'/' as usize] = (MODIFIER_NONE, 0x38); // /
    table[b'?' as usize] = (MODIFIER_SHIFT, 0x38); // ?
    table[b';' as usize] = (MODIFIER_NONE, 0x33); // ;
    table[b':' as usize] = (MODIFIER_SHIFT, 0x33); // :
    table[b'\'' as usize] = (MODIFIER_NONE, 0x34); // '
    table[b'"' as usize] = (MODIFIER_SHIFT, 0x34); // "

    table[b'[' as usize] = (MODIFIER_NONE, 0x2F); // [
    table[b'{' as usize] = (MODIFIER_SHIFT, 0x2F); // {
    table[b']' as usize] = (MODIFIER_NONE, 0x30); // ]
    table[b'}' as usize] = (MODIFIER_SHIFT, 0x30); // }
    table[b'\\' as usize] = (MODIFIER_NONE, 0x31); // \
    table[b'|' as usize] = (MODIFIER_SHIFT, 0x31); // |

    table[b'-' as usize] = (MODIFIER_NONE, 0x2D); // -
    table[b'_' as usize] = (MODIFIER_SHIFT, 0x2D); // _
    table[b'=' as usize] = (MODIFIER_NONE, 0x2E); // =
    table[b'+' as usize] = (MODIFIER_SHIFT, 0x2E); // +

    table[b'`' as usize] = (MODIFIER_NONE, 0x35); // `
    table[b'~' as usize] = (MODIFIER_SHIFT, 0x35); // ~

    // Shift Number Row Symbols
    table[b'!' as usize] = (MODIFIER_SHIFT, 0x1E); // !
    table[b'@' as usize] = (MODIFIER_SHIFT, 0x1F); // @
    table[b'#' as usize] = (MODIFIER_SHIFT, 0x20); // #
    table[b'$' as usize] = (MODIFIER_SHIFT, 0x21); // $
    table[b'%' as usize] = (MODIFIER_SHIFT, 0x22); // %
    table[b'^' as usize] = (MODIFIER_SHIFT, 0x23); // ^
    table[b'&' as usize] = (MODIFIER_SHIFT, 0x24); // &
    table[b'*' as usize] = (MODIFIER_SHIFT, 0x25); // *
    table[b'(' as usize] = (MODIFIER_SHIFT, 0x26); // (
    table[b')' as usize] = (MODIFIER_SHIFT, 0x27); // )

    table
};

pub fn char_to_hid(c: char) -> Option<KeyStroke> {
    let ascii_val = c as u32;
    if ascii_val < 128 {
        let (modifier, keycode) = ASCII_TO_HID[ascii_val as usize];
        if keycode != 0 {
            return Some(KeyStroke { modifier, keycode });
        }
    }
    None // Unmapped or invalid character
}
