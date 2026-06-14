use crate::buzzer::Melody;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver};

pub type MessageChannelType =
    Channel<CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, CHANNEL_SIZE>;
pub type MessageReceiver =
    Receiver<'static, CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, CHANNEL_SIZE>;
pub type SoundChannelType = Channel<CriticalSectionRawMutex, Option<Melody>, CHANNEL_SIZE>;
pub type SoundReceiver = Receiver<'static, CriticalSectionRawMutex, Option<Melody>, CHANNEL_SIZE>;
pub type CoordinatesChannelType = Channel<CriticalSectionRawMutex, Reading, CHANNEL_SIZE>;
pub type CoordinatesReceiver = Receiver<'static, CriticalSectionRawMutex, Reading, CHANNEL_SIZE>;
pub const MESSAGE_SIZE: usize = 512;
pub const CHANNEL_SIZE: usize = 10;
pub static MESSAGE_CHANNEL: MessageChannelType = Channel::new();
pub static SOUND_CHANNEL: SoundChannelType = Channel::new();
pub static COORDINATES_CHANNEL: CoordinatesChannelType = Channel::new();
pub static CHAR_CHANNEL: Channel<CriticalSectionRawMutex, char, CHANNEL_SIZE> = Channel::new();

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
