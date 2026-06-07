use crate::input::{value_to_percent, CHARSETS};
use crate::inter_task::{CoordinatesReceiver, MESSAGE_SIZE, MessageReceiver};
use crate::rainbow::{RAINBOW_RGB565_128, RAINBOW_RGB565_256, rgb565_rainbow};
use ariel_os::debug::log::{info, warn};
use ariel_os::time::{Instant, Timer};
use ariel_os_hal::gpio::Output;
use embassy_futures::select::{Either, select};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::primitives::{
    Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle,
};
use embedded_graphics::{
    geometry::Point,
    mono_font::{
        MonoTextStyle,
        iso_8859_5::{FONT_8X13_ITALIC, FONT_9X18, FONT_9X18_BOLD, FONT_10X20},
    },
    prelude::*,
    text::Text,
};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::spi::master::Spi;
use heapless::Deque;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorOrder, Rotation};
use mipidsi::{Builder, Display as DisplayImpl, models::ILI9341Rgb565, options::Orientation};

static BAND_HEIGHT: i32 = 30;

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

macro_rules! pos_to_y {
    ($value: expr, $band: expr) => {{
        let base = ((BAND_HEIGHT + 1) as f32 * $band as f32) as i32;
        let y = $value * (BAND_HEIGHT - 1) as f32 + 1.0;
        base + BAND_HEIGHT + 1 - y as i32
    }};
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

    #[allow(dead_code)]
    pub async fn print_text(&mut self, channel: MessageReceiver) {
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

    pub async fn draw_lines(&mut self, channel: CoordinatesReceiver) {
        let mut buffer_x0: Deque<i32, 350> = Deque::new();
        let mut buffer_y0: Deque<i32, 350> = Deque::new();
        let mut buffer_x1: Deque<i32, 350> = Deque::new();
        let mut buffer_y1: Deque<i32, 350> = Deque::new();
        let mut buffer_time: Deque<u64, 350> = Deque::new();
        self.display.clear(Rgb565::RED).unwrap();
        for band in 1..=3 {
            Line::new(
                Point::new(0, (BAND_HEIGHT + 1) * band),
                Point::new(320, (BAND_HEIGHT + 1) * band),
            )
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLACK, 1))
            .draw(&mut self.display)
            .unwrap();
        }
        loop {
            let coordinates = channel.receive().await;
            let start = Instant::now();
            let x0 = coordinates.x_0;
            let y0 = coordinates.y_0;
            let x1 = coordinates.x_1;
            let y1 = coordinates.y_1;

            // TODO: drawing two graphs on the same band flickers, need to make redraw accept
            //       several buffers and draw them all at once.
            redraw_and_fill(
                &mut self.display,
                Rgb565::YELLOW,
                pos_to_y!(x0, 0),
                &mut buffer_x0,
            );
            redraw_and_fill(
                &mut self.display,
                Rgb565::CSS_DARK_GREEN,
                pos_to_y!(y0, 0),
                &mut buffer_y0,
            );
            redraw_and_fill(
                &mut self.display,
                Rgb565::BLUE,
                pos_to_y!(x1, 1),
                &mut buffer_x1,
            );
            redraw_and_fill(
                &mut self.display,
                Rgb565::CSS_VIOLET,
                pos_to_y!(y1, 1),
                &mut buffer_y1,
            );
            draw_min_max(
                &mut self.display,
                "V",
                coordinates.min_v,
                coordinates.max_v,
                1,
            );
            let select = draw_position(&mut self.display, x1, y1, 4, 1, "");
            let charset = CHARSETS[ select ];
            draw_position(&mut self.display, x0, y0, 4, 0, charset);
            fill_and_draw_time(&mut self.display, start, &mut buffer_time, 2);
        }
    }
}

fn draw_position(
    display: &mut DisplayAlias<'_, '_>,
    x: f32,
    y: f32,
    band: usize,
    stick: usize,
    charset: &str,
) -> usize {
    let y_0 = (BAND_HEIGHT + 1) * band as i32 + 1;
    let diameter = 235 - y_0;
    let x_0 = 10 + (diameter + 10) * stick as i32;
    Rectangle::new(
        Point::new(x_0 - 1, y_0 - 1),
        Size::new(diameter as u32 + 6, diameter as u32 + 6),
    )
    .into_styled(PrimitiveStyleBuilder::new().fill_color(Rgb565::RED).build())
    .draw(display)
    .unwrap();
    Rectangle::new(
        Point::new(x_0, y_0),
        Size::new(diameter as u32, diameter as u32),
    )
    .into_styled(
        PrimitiveStyleBuilder::new()
            .stroke_width(1)
            .stroke_color(Rgb565::BLACK)
            .build(),
    )
    .draw(display)
    .unwrap();
    let selector_x;
    let selector_y;
    if x < 0.3 {
        selector_x = 0;
    } else if x < 0.7 {
        selector_x = 1;
    } else {
        selector_x = 2;
    }
    if y < 0.3 {
        selector_y = 2;
    } else if y < 0.7 {
        selector_y = 1;
    } else {
        selector_y = 0;
    }
    let border = if charset.len() > 0 && (x < 0.1 || y < 0.1 || x > 0.9 || y > 0.9) {
        3
    } else {
        1
    };
    Rectangle::new(
        Point::new(
            x_0 + diameter / 3 * selector_x,
            y_0 + diameter / 3 * selector_y,
        ),
        Size::new(diameter as u32 / 3, diameter as u32 / 3),
    )
    .into_styled(
        PrimitiveStyleBuilder::new()
            .stroke_width(border)
            .stroke_color(Rgb565::BLUE)
            .build(),
    )
    .draw(display)
    .unwrap();
    Circle::new(
        Point::new(
            x_0 + (x * diameter as f32) as i32,
            y_0 + ((1.0 - y) * diameter as f32) as i32,
        ),
        4,
    )
    .into_styled(
        PrimitiveStyleBuilder::new()
            .stroke_width(1)
            .stroke_color(Rgb565::BLACK)
            .fill_color(Rgb565::WHITE)
            .build(),
    )
    .draw(display)
    .unwrap();
    let mut char = heapless::String::<4>::new();
    for (index, ch) in charset.chars().enumerate() {
        let index = if index < 4 {
            index as i32
        } else {
            index as i32 + 1
        };
        let x_pos = x_0 + diameter / 3 * (index % 3) + diameter / 6 - 5;
        let y_pos = y_0 + diameter / 3 * (index / 3) + diameter / 6 + 4;
        if let Err(_) = core::fmt::write(&mut char, format_args!("{}", ch)) {
            warn!("Failed to write char: {}", ch);
            char.clear();
            continue;
        }
        Text::new(
            &char,
            Point::new(x_pos, y_pos),
            MonoTextStyle::new(&FONT_10X20, Rgb565::BLACK),
        )
        .draw(display)
        .unwrap();
        char.clear();
    }
    (selector_x + selector_y * 3) as usize
}

fn draw_min_max<T: core::fmt::Display>(
    display: &mut DisplayAlias<'_, '_>,
    prefix: &str,
    min: T,
    max: T,
    band: i32,
) {
    let y_0 = (BAND_HEIGHT + 1) * 3 + 1;
    let x_0 = 10 + band * 110;
    Rectangle::new(Point::new(x_0, y_0), Size::new(100, 16))
        .into_styled(PrimitiveStyleBuilder::new().fill_color(Rgb565::RED).build())
        .draw(display)
        .unwrap();
    let mut value = heapless::String::<8>::new();
    let Ok(_) = core::fmt::write(&mut value, format_args!("{}={:4}, ", prefix, min)) else {
        info!("Failed to write min");
        return;
    };
    let end = Text::new(
        &value,
        Point::new(x_0, y_0 + 15),
        MonoTextStyle::new(&FONT_8X13_ITALIC, Rgb565::BLACK),
    )
    .draw(display)
    .unwrap();
    value.clear();
    let Ok(_) = core::fmt::write(&mut value, format_args!("{}", max)) else {
        info!("Failed to write max");
        return;
    };
    Text::new(
        &value,
        end,
        MonoTextStyle::new(&FONT_8X13_ITALIC, Rgb565::BLACK),
    )
    .draw(display)
    .unwrap();
}

fn redraw_and_fill(
    display: &mut DisplayAlias<'_, '_>,
    color: Rgb565,
    value: i32,
    buffer: &mut Deque<i32, 350>,
) {
    let previous = buffer.clone();
    buffer.push_back(value).unwrap();
    if buffer.len() > 320 {
        buffer.pop_front();
    }
    for (index, (next, prev)) in buffer.iter().zip(previous.iter()).enumerate() {
        if index >= 320 {
            break;
        }
        if next != prev {
            Pixel(Point::new(index as i32, *prev), Rgb565::RED)
                .draw(display)
                .unwrap();
        }
    }
    draw_buffer(display, buffer, color);
}

fn draw_buffer(display: &mut DisplayAlias<'_, '_>, buffer: &Deque<i32, 350>, color: Rgb565) {
    for (index, value) in buffer.iter().enumerate() {
        Pixel(Point::new(index as i32, *value), color)
            .draw(display)
            .unwrap();
    }
}

/// Fill time buffer, decay min and max so that the startup min and max are not carried forever.
///
/// ```no_run
/// let mut buffer_time: Deque<i32, 350> = Deque::new();
/// let decay = 0.1;
/// let mut time = None;
/// let mut min_time = u64::MAX;
/// let mut max_time = u64::MIN;
///
/// draw_time_with_decay(&mut self.display, start, &mut buffer_time, &mut time, decay, &mut min_time, &mut max_time)
/// ```
#[allow(dead_code)]
fn fill_and_draw_time_with_decay(
    display: &mut DisplayAlias<'_, '_>,
    start: Instant,
    buffer_time: &mut Deque<i32, 350>,
    time: &mut Option<f32>,
    decay: f32,
    min_time: &mut u64,
    max_time: &mut u64,
) {
    if let Some(t) = time {
        redraw_and_fill(display, Rgb565::WHITE, pos_to_y!(*t, 4), buffer_time);
        draw_min_max(display, "t", *min_time, *max_time, 0);
    }
    let elapsed_ms = start.elapsed().as_millis();
    *min_time = (*min_time as f32 * (1.0 + decay)) as u64;
    *max_time = (*max_time as f32 * (1.0 - decay)) as u64;
    *time = Some(value_to_percent!(elapsed_ms, *min_time, *max_time, false));
}

/// Draw the time graph scaling it to currently visible min and max, this one is about 5 ms slower.
///
/// ```no_run
/// let mut buffer_time: Deque<u64, 350> = Deque::new();
///
/// draw_time(&mut self.display, start, &mut buffer_time);
/// ```
#[allow(dead_code)]
fn fill_and_draw_time(
    display: &mut DisplayAlias<'_, '_>,
    start: Instant,
    buffer_time: &mut Deque<u64, 350>,
    band: usize,
) {
    Rectangle::new(
        Point::new(0, (BAND_HEIGHT + 1) * band as i32 + 1),
        Size::new(320, BAND_HEIGHT as u32),
    )
    .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
    .draw(display)
    .unwrap();
    if let Some((min, max)) = draw_buffer_scaled(display, &buffer_time, band, Rgb565::WHITE) {
        draw_min_max(display, "t", min, max, 0);
    }
    let elapsed_ms = start.elapsed().as_millis();
    buffer_time.push_back(elapsed_ms).unwrap();
    if buffer_time.len() > 320 {
        buffer_time.pop_front();
    }
}

fn draw_buffer_scaled(
    display: &mut DisplayAlias<'_, '_>,
    input_buffer: &Deque<u64, 350>,
    band: usize,
    color: Rgb565,
) -> Option<(u64, u64)> {
    if input_buffer.len() < 1 {
        return None;
    }
    let mut min = u64::MAX;
    let mut max = u64::MIN;
    for value in input_buffer.iter() {
        if *value < min {
            min = *value;
        }
        if *value > max {
            max = *value;
        }
    }
    for (index, raw_value) in input_buffer.iter().enumerate() {
        let value = pos_to_y!(value_to_percent!(*raw_value, min, max, false), band);
        Pixel(Point::new(index as i32, value), color)
            .draw(display)
            .unwrap();
    }
    Some((min, max))
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
                char.clear();
                continue;
            };
            let mut next_point = Text::new(&char, point, text_style).draw(display).unwrap();
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
        point = Text::new(&char, point, text_style).draw(display).unwrap();
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
            point = Text::new(&char, point, text_style).draw(display).unwrap();
        }
        char.clear();
        Timer::after_millis(50).await;
    }

    display.clear(Rgb565::BLACK).unwrap();
}
