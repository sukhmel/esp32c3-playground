use crate::buzzer::Melody;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver};

pub type MessageChannelType =
    Channel<CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, CHANNEL_SIZE>;
pub type MessageReceiver =
    Receiver<'static, CriticalSectionRawMutex, heapless::String<MESSAGE_SIZE>, CHANNEL_SIZE>;
pub type SoundChannelType = Channel<CriticalSectionRawMutex, Option<Melody>, CHANNEL_SIZE>;
pub type SoundReceiver = Receiver<'static, CriticalSectionRawMutex, Option<Melody>, CHANNEL_SIZE>;
pub const MESSAGE_SIZE: usize = 512;
pub const CHANNEL_SIZE: usize = 2;
pub static MESSAGE_CHANNEL: MessageChannelType = Channel::new();
pub static SOUND_CHANNEL: SoundChannelType = Channel::new();
