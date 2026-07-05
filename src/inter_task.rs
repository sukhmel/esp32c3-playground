use crate::buzzer::Melody;
use crate::touch::TouchInputResponse;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver};
use embassy_sync::watch::{Receiver as WatchReceiver, Watch};

pub type MessageChannelType =
    Channel<CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, CHANNEL_SIZE>;
pub type MessageReceiver =
    Receiver<'static, CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, CHANNEL_SIZE>;
pub type SoundChannelType = Channel<CriticalSectionRawMutex, Option<Melody>, CHANNEL_SIZE>;
pub type SoundReceiver = Receiver<'static, CriticalSectionRawMutex, Option<Melody>, CHANNEL_SIZE>;
pub type CoordinatesChannelType = Channel<CriticalSectionRawMutex, Reading, LARGE_CHANNEL_SIZE>;
pub type CoordinatesReceiver =
    Receiver<'static, CriticalSectionRawMutex, Reading, LARGE_CHANNEL_SIZE>;
pub type TouchChannelType =
    Channel<CriticalSectionRawMutex, TouchInputResponse, LARGE_CHANNEL_SIZE>;
pub type TouchReceiver =
    Receiver<'static, CriticalSectionRawMutex, TouchInputResponse, LARGE_CHANNEL_SIZE>;
pub type CharChannelType = Channel<CriticalSectionRawMutex, char, CHANNEL_SIZE>;
pub type CharReceiver = Receiver<'static, CriticalSectionRawMutex, char, CHANNEL_SIZE>;
pub type KeypressChannelType = Channel<CriticalSectionRawMutex, Keypress, LARGE_CHANNEL_SIZE>;
pub type KeypressReceiver =
    Receiver<'static, CriticalSectionRawMutex, Keypress, LARGE_CHANNEL_SIZE>;

/// Number of independent consumers of a [`Watch`]. Keep in sync with `.receiver()` calls.
pub const WATCH_CONSUMERS: usize = 2;

/// Latest-value channel for the "IP:port" line shown on screen. Unlike a
/// [`Channel`], a [`Watch`] keeps only the most recent value, so a slow display
/// consumer can never make the producer block or drop the current address.
pub type IpDisplayWatch = Watch<CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, WATCH_CONSUMERS>;
pub type IpDisplayReceiver =
    WatchReceiver<'static, CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, WATCH_CONSUMERS>;

/// Latest-value channel carrying BLE connection state (`true` = a central is
/// connected). The Wi-Fi side observes this to stand down while BLE is active,
/// since the ESP32-C3 shares a single 2.4 GHz radio between Wi-Fi and BLE.
pub type BleStateWatch = Watch<CriticalSectionRawMutex, bool, WATCH_CONSUMERS>;
pub type BleStateReceiver =
    WatchReceiver<'static, CriticalSectionRawMutex, bool, WATCH_CONSUMERS>;

pub const MESSAGE_SIZE: usize = 128;
pub const CHANNEL_SIZE: usize = 2;
pub const LARGE_CHANNEL_SIZE: usize = 10;
pub static MESSAGE_CHANNEL: MessageChannelType = Channel::new();
pub static SOUND_CHANNEL: SoundChannelType = Channel::new();
pub static COORDINATES_CHANNEL: CoordinatesChannelType = Channel::new();
pub static CHAR_CHANNEL: CharChannelType = Channel::new();
pub static KEYPRESS_CHANNEL: KeypressChannelType = Channel::new();
pub static TOUCH_CHANNEL: TouchChannelType = Channel::new();
pub static IP_DISPLAY: IpDisplayWatch = Watch::new();
pub static BLE_CONNECTED: BleStateWatch = Watch::new();

#[derive(Debug)]
pub struct Reading {
    pub v_x_0: u16,
    pub v_y_0: u16,
    pub v_x_1: u16,
    pub v_y_1: u16,
    pub x_0: f32,
    pub y_0: f32,
    pub x_1: f32,
    pub y_1: f32,
    pub min_v: u16,
    pub max_v: u16,
    pub us: u64,
    pub sel_x_0: i8,
    pub sel_y_0: i8,
    pub sel_x_1: i8,
    pub sel_y_1: i8,
    pub pressed: bool,
}

impl Default for Reading {
    fn default() -> Self {
        Self {
            v_x_0: 0,
            v_y_0: 0,
            v_x_1: 0,
            v_y_1: 0,
            x_0: 0.0,
            y_0: 0.0,
            x_1: 0.0,
            y_1: 0.0,
            min_v: 0,
            max_v: 0,
            us: 0,
            sel_x_0: 0,
            sel_y_0: 0,
            sel_x_1: 0,
            sel_y_1: 0,
            pressed: false,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Keypress {
    Pressed(char),
    Released(char),
}
