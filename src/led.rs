use crate::pins::Peripherals;
use ariel_os::debug::log::info;
use ariel_os::time::Timer;
use ariel_os_hal::gpio::{Level, Output};
use core::iter;
use esp_hal::rmt::Rmt;
use esp_hal::time::Rate;
use esp_hal_smartled::{SmartLedsAdapter, smart_led_buffer};
use smart_leds::{RGB8, SmartLedsWrite};

#[allow(dead_code)]
async fn cycle_rgb_led(peripherals: Peripherals) {
    let rmt = Rmt::new(peripherals.rmt, Rate::from_mhz(80)).unwrap();
    let rmt_channel = rmt.channel0;
    let mut rmt_buffer = smart_led_buffer!(1);
    let mut led_strip = SmartLedsAdapter::new(rmt_channel, peripherals.pin8, &mut rmt_buffer);
    for index in 0..=16 {
        let a = index % 4;
        let b = (index >> 2) % 4;
        let (ra, ga, ba) = match a {
            0 => (0, 0, 0),
            1 => (1, 0, 0),
            2 => (0, 1, 0),
            3 => (0, 0, 1),
            _ => unreachable!(),
        };
        let (rb, gb, bb) = match b {
            0 => (0, 0, 0),
            1 => (1, 0, 0),
            2 => (0, 1, 0),
            3 => (0, 0, 1),
            _ => unreachable!(),
        };
        let r = (ra + rb) * 8;
        let g = (ga + gb) * 8;
        let b = (ba + bb) * 8;
        led_strip.write(iter::once(RGB8 { r, g, b })).unwrap();
        info!("LED: R={} G={} B={}", r, g, b);
        Timer::after_millis(1000).await;
    }
}

#[allow(dead_code)]
async fn led_counter(peripherals: Peripherals) {
    let mut led0 = Output::new(peripherals.pin0, Level::Low);
    let mut led1 = Output::new(peripherals.pin1, Level::Low);
    let mut led2 = Output::new(peripherals.pin2, Level::Low);
    let mut led3 = Output::new(peripherals.pin3, Level::Low);
    let mut led4 = Output::new(peripherals.pin4, Level::Low);
    let mut led5 = Output::new(peripherals.pin5, Level::Low);
    let mut led6 = Output::new(peripherals.pin6, Level::Low);
    let mut led7 = Output::new(peripherals.pin7, Level::Low);
    let mut led8 = Output::new(peripherals.pin10, Level::Low);
    for index in 1022..=1024 {
        if index % 2 == 0 {
            led0.set_level(Level::Low);
        } else {
            led0.set_level(Level::High);
        }
        if (index >> 1) % 2 == 0 {
            led1.set_level(Level::Low);
        } else {
            led1.set_level(Level::High);
        }
        if (index >> 2) % 2 == 0 {
            led2.set_level(Level::Low);
        } else {
            led2.set_level(Level::High);
        }
        if (index >> 3) % 2 == 0 {
            led3.set_level(Level::Low);
        } else {
            led3.set_level(Level::High);
        }
        if (index >> 4) % 2 == 0 {
            led4.set_level(Level::Low);
        } else {
            led4.set_level(Level::High);
        }
        if (index >> 5) % 2 == 0 {
            led5.set_level(Level::Low);
        } else {
            led5.set_level(Level::High);
        }
        if (index >> 6) % 2 == 0 {
            led6.set_level(Level::Low);
        } else {
            led6.set_level(Level::High);
        }
        if (index >> 7) % 2 == 0 {
            led7.set_level(Level::Low);
        } else {
            led7.set_level(Level::High);
        }
        if (index >> 8) % 2 == 0 {
            led8.set_level(Level::Low);
        } else {
            led8.set_level(Level::High);
        }
        Timer::after_millis(500).await;
    }
}
