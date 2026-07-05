use crate::inter_task::{CHAR_CHANNEL, COORDINATES_CHANNEL, Reading, KEYPRESS_CHANNEL, Keypress};
use crate::pins::AnalogPeripherals;
use ariel_os::debug::log::{debug, warn};
use ariel_os::time::{Instant, Timer};
use esp_hal::analog::adc::{Adc, AdcConfig, Attenuation};

static DEFAULT_MIN_V: u16 = 1620;
static DEFAULT_MAX_V: u16 = 3860;

// 0 1 2
// 3 4 5
// 6 7 8
pub static CHARSETS: [&str; 9] = [
    ",M._/[`]",
    "RTYFGHCV",
    "UIOJKLBN",
    "{P}<>:'\"",
    "QWEADZSX",
    "12345678",
    "90!@#$%^",
    "(&)*-+=\\",
    "[ ]{}|;~",
];

macro_rules! value_to_percent {
    ($value: expr, $min: expr, $max: expr, $invert: expr) => {{
        if $value < $min {
            $min = $value;
        }
        if $value > $max {
            $max = $value;
        }
        if $max == $min {
            0.5
        } else {
            let pos = (($value - $min) as f32) / ($max - $min) as f32;
            if $invert { 1.0 - pos } else { pos }
        }
    }};
}

pub(crate) use value_to_percent;

pub async fn read_joystick(peripherals: AnalogPeripherals) {
    let mut min_v = DEFAULT_MIN_V;
    let mut max_v = DEFAULT_MAX_V;
    let mut adc1_config = AdcConfig::new();
    let mut x_0 = adc1_config.enable_pin(peripherals.pin3, Attenuation::_11dB);
    let mut y_0 = adc1_config.enable_pin(peripherals.pin2, Attenuation::_11dB);
    let mut x_1 = adc1_config.enable_pin(peripherals.pin1, Attenuation::_11dB);
    let mut y_1 = adc1_config.enable_pin(peripherals.pin0, Attenuation::_11dB);
    let mut adc1 = Adc::new(peripherals.adc1, adc1_config);
    let mut keypress = None;
    let mut keypress_cycle = 0;

    loop {
        let start = Instant::now();
        let x_0_value = nb::block!(adc1.read_oneshot(&mut x_0)).unwrap();
        let y_0_value = nb::block!(adc1.read_oneshot(&mut y_0)).unwrap();
        let x_1_value = nb::block!(adc1.read_oneshot(&mut x_1)).unwrap();
        let y_1_value = nb::block!(adc1.read_oneshot(&mut y_1)).unwrap();

        let elapsed = start.elapsed().as_micros();
        let x_0 = value_to_percent!(x_0_value, min_v, max_v, false);
        let y_0 = value_to_percent!(y_0_value, min_v, max_v, true);
        let x_1 = value_to_percent!(x_1_value, min_v, max_v, false);
        let y_1 = value_to_percent!(y_1_value, min_v, max_v, true);
        let sel_x_0 = if x_0 < 0.3 {
            0
        } else if x_0 < 0.7 {
            1
        } else {
            2
        };
        let sel_y_0 = if y_0 < 0.3 {
            2
        } else if y_0 < 0.7 {
            1
        } else {
            0
        };
        let sel_x_1 = if x_1 < 0.3 {
            0
        } else if x_1 < 0.7 {
            1
        } else {
            2
        };
        let sel_y_1 = if y_1 < 0.3 {
            2
        } else if y_1 < 0.7 {
            1
        } else {
            0
        };

        let charset = CHARSETS[(sel_x_1 + sel_y_1 * 3) as usize % CHARSETS.len()];
        if x_0 < 0.05 || y_0 < 0.05 || x_0 > 0.95 || y_0 > 0.95 {
            let select = sel_x_0 + sel_y_0 * 3;
            let skip = if select < 4 { select } else { select - 1 };
            let Some(char) = charset.chars().skip(skip as usize).next() else {
                keypress_cycle = 0;
                continue;
            };
            match keypress {
                None => {
                    debug!("Key pressed: {}", char);
                    if CHAR_CHANNEL.try_send(char).is_err() {
                        warn!("Failed to send keypress");
                    }
                    let _ = KEYPRESS_CHANNEL.try_send(Keypress::Pressed(char));
                    keypress = Some(char);
                    keypress_cycle = 0;
                }
                Some(prev_char) if char != prev_char => {
                    debug!("Key released: {}", prev_char);
                    debug!("Key pressed: {}", char);
                    if CHAR_CHANNEL.try_send(char).is_err() {
                        warn!("Failed to send keypress");
                    }
                    let _ = KEYPRESS_CHANNEL.try_send(Keypress::Released(prev_char));
                    let _ = KEYPRESS_CHANNEL.try_send(Keypress::Pressed(char));
                    keypress = Some(char);
                    keypress_cycle = 0;
                }
                Some(char) => {
                    keypress_cycle += 1;
                    if keypress_cycle > 5 && keypress_cycle % 2 == 0 {
                        debug!("Key repeated: {}", char);
                        if CHAR_CHANNEL.try_send(char).is_err() {
                            warn!("Failed to send keypress");
                        }
                        let _ = KEYPRESS_CHANNEL.try_send(Keypress::Pressed(char));
                    }
                }
            }
        } else {
            if let Some(char) = keypress.take() {
                let _ = KEYPRESS_CHANNEL.try_send(Keypress::Released(char));
            }
            keypress_cycle = 0;
        }

        if let Err(_) = COORDINATES_CHANNEL.try_send(Reading {
            v_x_0: x_0_value,
            v_y_0: y_0_value,
            v_x_1: x_1_value,
            v_y_1: y_1_value,
            x_0,
            y_0,
            x_1,
            y_1,
            sel_x_0,
            sel_y_0,
            sel_x_1,
            sel_y_1,
            min_v,
            max_v,
            us: elapsed,
            pressed: keypress.is_some(),
        }) {
            warn!("Failed to send coordinates");
        }

        Timer::after_millis(100).await;
    }
}
