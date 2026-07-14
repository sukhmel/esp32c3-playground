use crate::inter_task::{CHAR_CHANNEL, COORDINATES_CHANNEL, KEYPRESS_CHANNEL, Keypress, Reading, ButtonState, BUTTON_STATE_SIGNAL};
use crate::pins::AnalogPeripherals;
use ariel_os::debug::log::{debug, info, warn};
use ariel_os::time::{Instant, Timer};
use embassy_futures::select::{select, Either};
use esp_hal::analog::adc::{Adc, AdcConfig, Attenuation};
use esp_hal::gpio::{Event, Input};

static DEFAULT_MIN_V: u16 = 1620;
static DEFAULT_MAX_V: u16 = 3860;

// Characters that stand for special keys inside [`CHARSETS`]. Shared by the
// display (draws a glyph, see `display::draw_special_glyph`) and the keyboard
// (emits a keycode, see `keyboard::SPECIAL_KEYS`) so the two never disagree.
pub const CH_TAB: char = '\t'; // 0x09
pub const CH_ENTER: char = '\n'; // 0x0A
pub const CH_BACKSPACE: char = '\u{8}'; // 0x08, i.e. \b
pub const CH_ESCAPE: char = '\u{1b}'; // 0x1B, i.e. \e
pub const CH_DELETE: char = '\u{7f}'; // Delete Forward
pub const CH_LEFT_ARROW: char = '\u{2190}'; // ← Left Arrow
pub const CH_RIGHT_ARROW: char = '\u{2192}'; // → Right Arrow
pub const CH_UP_ARROW: char = '\u{2191}'; // ↑ Up Arrow
pub const CH_DOWN_ARROW: char = '\u{2193}'; // ↓ Down Arrow

// 0 1 2
// 3 4 5
// 6 7 8
pub static CHARSETS: [&str; 9] = [
    ",M._\u{1b}/[`]",
    "RTYF↑GHCV",
    "UIOJ\nKLBN",
    "{P}<←>:'\"",
    "QWEA DZSX",
    "1234→5678",
    "90!@ #$%^",
    "(&)*↓-+=\\",
    "[\u{8}]{\t}|;~",
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

pub async fn read_joystick(peripherals: AnalogPeripherals, mut button: Input<'_>) {
    info!("input: task started");
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
    let mut button_pressed = false;

    loop {
        match select(
            button.wait_for(if button_pressed {
                Event::HighLevel
            } else {
                Event::LowLevel
            }),
            Timer::after_millis(100),
        ).await {
            Either::Second(_) => {}
            Either::First(()) => {
                button_pressed = !button_pressed;

                if button_pressed {
                    info!("Set button pressed");
                } else {
                    keypress_cycle = 0;
                    if let Some(char) = keypress.take() {
                        debug!("Key released: {}", char);
                        let _ = KEYPRESS_CHANNEL.try_send(Keypress::Released(char));
                    }
                }
            }
        }

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
        // when button is pressed, select the center character
        let sel_x_0 = if button_pressed {
            1
        } else if x_0 < 0.3 {
            0
        } else if x_0 < 0.7 {
            1
        } else {
            2
        };
        let sel_y_0 = if button_pressed {
            1
        } else if y_0 < 0.3 {
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
        if button_pressed {
            let char = charset.chars().nth(4).unwrap();
            match keypress.replace(char) {
                Some(prev_char) if char != prev_char => {
                    debug!("Key released: {}", prev_char);
                    let _ = KEYPRESS_CHANNEL.try_send(Keypress::Released(prev_char));
                }
                None => {
                    debug!("Key stored: {}", char);
                    keypress = Some(char);
                    // since pressing a button takes more effort, there is no need for grace period.
                    keypress_cycle = 2;
                }
                _ => {}
            }
        } else if x_0 < 0.05 || y_0 < 0.05 || x_0 > 0.95 || y_0 > 0.95 {
            let select = sel_x_0 + sel_y_0 * 3;
            let Some(char) = charset.chars().nth(select as usize) else {
                keypress_cycle = 0;
                continue;
            };
            match keypress {
                None => {
                    debug!("Key stored: {}", char);
                    keypress = Some(char);
                    keypress_cycle = 0;
                }
                Some(prev_char) if char != prev_char => {
                    debug!("Key released: {}", prev_char);
                    debug!("Key stored: {}", char);
                    let _ = KEYPRESS_CHANNEL.try_send(Keypress::Released(prev_char));
                    keypress = Some(char);
                    keypress_cycle = 0;
                }
                Some(_char) => {
                    keypress_cycle += 1;
                }
            }
        } else {
            if let Some(char) = keypress.take() {
                let _ = KEYPRESS_CHANNEL.try_send(Keypress::Released(char));
            }
            keypress_cycle = 0;
        }

        if keypress_cycle == 2 || keypress_cycle > 9 && keypress_cycle % 3 == 0 {
            if let Some(char) = keypress {
                debug!("Key pressed: {}", char);
                if CHAR_CHANNEL.try_send(char).is_err() {
                    debug!("Failed to send keypress");
                }
                let _ = KEYPRESS_CHANNEL.try_send(Keypress::Pressed(char));
            }
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
    }
}
