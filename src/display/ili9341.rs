use crate::inter_task::{MESSAGE_SIZE, MessageReceiver};
use ariel_os::debug::log::{info, warn};
use ariel_os::time::{Instant, Timer};
use ariel_os_hal::gpio::Output;
use embassy_futures::select::{Either, select};
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::{
    mono_font::{
        MonoTextStyle,
        iso_8859_5::{FONT_8X13_ITALIC, FONT_9X18, FONT_9X18_BOLD},
    },
    prelude::*,
    text::Text,
};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::spi::master::Spi;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorOrder, Rotation};
use mipidsi::{Builder, Display as DisplayImpl, models::ILI9341Rgb565, options::Orientation};

include!(concat!(env!("OUT_DIR"), "/rainbows.rs"));

fn rainbow_at(length: usize, step: usize) -> Rgb565 {
    let step = step % length;

    if length == 128 {
        return Rgb565::from(RawU16::from(RAINBOW_RGB565_128[step]));
    }
    if length == 256 {
        return Rgb565::from(RawU16::from(RAINBOW_RGB565_256[step]));
    }

    rgb565_rainbow(step, length)
}

type DisplayAlias<'a, 'd> = DisplayImpl<
    SpiInterface<'a, ExclusiveDevice<Spi<'d, Blocking>, Output, Delay>, Output>,
    ILI9341Rgb565,
    Output,
>;

pub struct Display<'a, 'd> {
    display: DisplayAlias<'a, 'd>,
}

impl<'a, 'd> Display<'a, 'd> {
    pub fn new(
        raw_spi: Spi<'d, Blocking>,
        cs_pin: Output,
        dc_pin: Output,
        rst_pin: Output,
        buffer: &'a mut [u8; 512],
    ) -> Self {
        let spi = ExclusiveDevice::new(raw_spi, cs_pin, Delay::new()).unwrap();
        let di = SpiInterface::new(spi, dc_pin, buffer.as_mut_slice());
        let mut delay = Delay::new();
        let display = Builder::new(ILI9341Rgb565, di)
            .reset_pin(rst_pin)
            .orientation(Orientation::new().rotate(Rotation::Deg270).flip_vertical())
            .color_order(ColorOrder::Bgr)
            .init(&mut delay)
            .expect("Failed to initialize modern ILI9341 driver");
        Self { display }
    }

    pub async fn control_display(&mut self, channel: MessageReceiver) {
        self.display.clear(Rgb565::BLACK).unwrap();
        let mut text = None;
        loop {
            match select(rainbow_text(&mut self.display, &text), channel.receive()).await {
                Either::First(_) => {}
                Either::Second(message) => {
                    text = Some(message);
                }
            }
        }
    }
}

async fn rainbow_text(
    display: &mut DisplayAlias<'_, '_>,
    text: &Option<heapless::String<MESSAGE_SIZE>>,
) {
    let Some(text) = text else {
        Timer::after_ticks(u64::MAX / 2).await;
        return;
    };
    display.clear(Rgb565::BLACK).unwrap();
    let origin = Point::new(10, 10);
    let mut max_duration = 100;
    let mut redraw_index = 0;
    let mut clock = Instant::now();
    let mut last_point = origin;
    let mut color = 0;
    let mut char = heapless::String::<4>::new();
    for i in 0.. {
        if clock.elapsed().as_millis() > max_duration {
            color += 1;
            redraw_index = 0;
            last_point = origin;
            clock = Instant::now();
        }
        let mut point = last_point;
        let start = Instant::now();
        for (j, ch) in text.chars().enumerate() {
            if j > i || j < redraw_index || last_point.y > 232 {
                break;
            } else {
                redraw_index = j + 1;
            }
            let text_style = MonoTextStyle::new(
                &FONT_8X13_ITALIC,
                rainbow_at(
                    128,
                    usize::MAX / 2 - color * 10 + point.x as usize + point.y as usize,
                ),
            );
            let Ok(_) = core::fmt::write(&mut char, format_args!("{}", ch)) else {
                info!("Failed to write char: {}", ch);
                continue;
            };
            let mut next_point = Text::new(&char, point, text_style)
                .draw(display)
                .unwrap();
            char.clear();
            let continued = if next_point.x > 310 {
                next_point = Text::new("\n", next_point, text_style)
                    .draw(display)
                    .unwrap();
                true
            } else {
                false
            };
            if next_point.y > point.y {
                point.x = origin.x + if continued { 10 } else { 0 };
                point.y = next_point.y;
            } else {
                point = next_point;
            }
            last_point = next_point;
        }
        let elapsed_ms = start.elapsed().as_millis();
        if elapsed_ms > max_duration {
            max_duration = elapsed_ms;
            warn!("Text rendering took {} ms", elapsed_ms);
        }
        Timer::after_millis(50u64.checked_sub(elapsed_ms).unwrap_or(0).max(10)).await;
    }
}

#[allow(dead_code)]
pub async fn test_display(display: &mut DisplayAlias<'_, '_>) {
    display.clear(Rgb565::RED).unwrap();

    Timer::after_secs(1).await;

    let mut char = heapless::String::<4>::new();

    let text_style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::GREEN);
    let mut point = Point::new(10, 30);
    for ch in "Ariel OS + ESP32-C3 + ILI9341".chars() {
        let Ok(_) = core::fmt::write(&mut char, format_args!("{}", ch)) else {
            continue;
        };
        point = Text::new(&char, point, text_style)
            .draw(display)
            .unwrap();
        char.clear();
        Timer::after_millis(20).await;
    }
    let mut point = Point::new(10, 50);
    let text_style = MonoTextStyle::new(&FONT_9X18, Rgb565::GREEN);
    for ch in "(sync display driver)".chars() {
        let Ok(_) = core::fmt::write(&mut char, format_args!("{}", ch)) else {
            continue;
        };
        point = Text::new(&char, Point::new(point.x, point.y), text_style)
            .draw(display)
            .unwrap();
        char.clear();
        Timer::after_millis(20).await;
    }

    let shift = 5;
    let steps = 16;
    let origin = Point::new(10, 70);
    for i in (0..=steps).rev() {
        let text_style = MonoTextStyle::new(
            &FONT_8X13_ITALIC,
            Rgb565::new(20, i * 2 + 30, (steps - i) + 16),
        );
        Text::new(
            "тест кириллического текста",
            Point::new(origin.x + i as i32 * shift, origin.y + i as i32 * shift),
            text_style,
        )
        .draw(display)
        .unwrap();
        Timer::after_millis(100).await;
    }

    for i in (0..512).rev() {
        let mut point = origin;
        for (j, ch) in "тест кириллического текста".chars().enumerate() {
            let text_style =
                MonoTextStyle::new(&FONT_8X13_ITALIC, rainbow_at(128, i * 10 + j * 10));
            let Ok(_) = core::fmt::write(&mut char, format_args!("{}", ch)) else {
                continue;
            };
            point = Text::new(&char, point, text_style)
                .draw(display)
                .unwrap();
        }
        char.clear();
        Timer::after_millis(50).await;
    }

    display.clear(Rgb565::BLACK).unwrap();
}
