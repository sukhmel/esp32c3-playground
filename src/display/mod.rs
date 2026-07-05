#[cfg(feature = "agm1264f")]
pub mod agm1264f;
#[cfg(not(feature = "async_ili9341"))]
pub mod ili9341;
#[cfg(feature = "async_ili9341")]
pub mod ili9341_async;

use crate::input::{CHARSETS, value_to_percent};
use crate::inter_task::{
    CoordinatesReceiver, IpDisplayReceiver, MESSAGE_SIZE, MessageReceiver, Reading, TouchReceiver,
};
use crate::rainbow::{RAINBOW_RGB565_128, RAINBOW_RGB565_256, rgb565_rainbow};
use crate::touch::TouchInputResponse;
use ariel_os::debug::log::{info, warn};
use ariel_os::time::{Duration, Instant, Timer};
use core::fmt::Debug;
use embassy_futures::select::{Either, select};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::pixelcolor::raw::RawU16;
use embedded_graphics::primitives::{
    Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle,
};
use embedded_graphics::{
    draw_target::DrawTargetExt,
    geometry::Point,
    mono_font::{
        MonoTextStyle,
        iso_8859_5::{
            FONT_6X10, FONT_8X13_BOLD, FONT_8X13_ITALIC, FONT_9X18, FONT_9X18_BOLD, FONT_10X20,
        },
    },
    prelude::*,
    text::Text,
};
use embedded_graphics_framebuf::FrameBuf;
use heapless::Deque;
#[cfg(not(feature = "async_ili9341"))]
pub use ili9341::Display;
#[cfg(feature = "async_ili9341")]
pub use ili9341_async::Display;

static INPUT_COLORS: [Rgb565; 4] = [
    Rgb565::BLUE,
    Rgb565::CSS_VIOLET,
    Rgb565::YELLOW,
    Rgb565::CSS_DARK_GREEN,
];
static BAND_HEIGHT: i32 = 30;
static POSITION_PAD_DIAMETER: usize = 240usize
    .checked_sub(4 * (BAND_HEIGHT + 1) as usize)
    .unwrap();

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
    // map to [0..(BAND_HEIGHT - 1)] so that drawing in box of BAND_HEIGHT height works, invert y
    ($value: expr) => {{ BAND_HEIGHT - 1 - ($value * (BAND_HEIGHT - 1) as f32) as i32 }};
}

pub trait DisplayTarget {
    async fn clear(&mut self, color: Rgb565) -> Result<(), ()>;
    async fn draw(
        &mut self,
        origin: Point,
        size: Size,
        pixels: impl IntoIterator<Item = Rgb565>,
    ) -> Result<(), ()>;
}

#[allow(dead_code)]
pub async fn print_text<T: DrawTarget<Color = Rgb565>>(display: &mut T, channel: MessageReceiver)
where
    <T as DrawTarget>::Error: Debug,
{
    display.clear(Rgb565::BLACK).unwrap();
    let mut text = None;
    loop {
        match select(rainbow_text(display, &text), channel.receive()).await {
            Either::First(_) => {}
            Either::Second(message) => {
                text = Some(message);
            }
        }
    }
}

pub async fn debug_input<T: DisplayTarget>(
    display: &mut T,
    channel: CoordinatesReceiver,
    mut address: IpDisplayReceiver,
    touch: TouchReceiver,
) {
    info!("display: debug task started");
    let mut message = None;
    let mut frame_buffer_data = [Rgb565::RED; (320 * BAND_HEIGHT) as usize];
    let mut frame_buffer = FrameBuf::new(&mut frame_buffer_data, 320, BAND_HEIGHT as usize);
    let mut buffer_touch: Deque<TouchInputResponse, 100> = Deque::new();
    let mut buffer_x0: Deque<i32, 321> = Deque::new();
    let mut buffer_y0: Deque<i32, 321> = Deque::new();
    let mut buffer_x1: Deque<i32, 321> = Deque::new();
    let mut buffer_y1: Deque<i32, 321> = Deque::new();
    let mut buffer_time: Deque<u64, 321> = Deque::new();
    display.clear(Rgb565::RED).await.unwrap();
    Line::new(Point::new(0, 0), Point::new(320, 0))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLACK, 1))
        .draw(&mut frame_buffer)
        .unwrap();
    for band in 1..=3 {
        display
            .draw(
                Point::new(0, (BAND_HEIGHT + 1) * band - 1),
                Size::new(320, 1),
                frame_buffer.data.iter().copied(),
            )
            .await
            .unwrap();
    }
    let mut time = None;
    let mut min_v = u16::MAX;
    let mut max_v = u16::MIN;
    let mut x_0 = 0.0;
    let mut y_0 = 0.0;
    let mut current_coordinates = Reading::default();
    let mut current_select = 4;
    loop {
        let start = Instant::now();
        let mut loaded = 0;
        while let Ok(coordinates) = channel.try_receive() {
            loaded += 1;
            let x0 = coordinates.x_0;
            let y0 = coordinates.y_0;
            let x1 = coordinates.x_1;
            let y1 = coordinates.y_1;
            current_coordinates = coordinates;

            buffer_x0.push_back(pos_to_y!(x0)).unwrap();
            if buffer_x0.len() > 320 {
                buffer_x0.pop_front();
            }
            buffer_y0.push_back(pos_to_y!(y0)).unwrap();
            if buffer_y0.len() > 320 {
                buffer_y0.pop_front();
            }
            buffer_x1.push_back(pos_to_y!(x1)).unwrap();
            if buffer_x1.len() > 320 {
                buffer_x1.pop_front();
            }
            buffer_y1.push_back(pos_to_y!(y1)).unwrap();
            if buffer_y1.len() > 320 {
                buffer_y1.pop_front();
            }
        }
        while let Ok(touch) = touch.try_receive() {
            loaded += 1;
            match &touch {
                TouchInputResponse::Moved { x, y } => {
                    info!("touch: moved {} {}", x, y);
                }
                TouchInputResponse::Pressed { x, y } => {
                    info!("touch: pressed {} {}", x, y);
                }
                TouchInputResponse::Released { x, y } => {
                    info!("touch: released {} {}", x, y);
                }
                _ => {}
            }
            buffer_touch.push_back(touch).unwrap();

            if buffer_touch.len() > 99 {
                buffer_touch.pop_front();
            }
        }
        if loaded == 0 {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        }
        draw_buffer(&mut frame_buffer, &buffer_x1, INPUT_COLORS[2]);
        draw_buffer(&mut frame_buffer, &buffer_y1, INPUT_COLORS[3]);
        draw_touch_buffer(&mut frame_buffer, &mut buffer_touch, 0, Rgb565::WHITE);
        display
            .draw(
                Point::new(0, 0),
                frame_buffer.size(),
                frame_buffer.data.iter().copied(),
            )
            .await
            .unwrap();
        frame_buffer.clear(Rgb565::RED).unwrap();

        draw_buffer(&mut frame_buffer, &buffer_x0, INPUT_COLORS[0]);
        draw_buffer(&mut frame_buffer, &buffer_y0, INPUT_COLORS[1]);
        draw_touch_buffer(
            &mut frame_buffer,
            &mut buffer_touch,
            BAND_HEIGHT,
            Rgb565::WHITE,
        );
        display
            .draw(
                Point::new(0, BAND_HEIGHT + 1),
                frame_buffer.size(),
                frame_buffer.data.iter().copied(),
            )
            .await
            .unwrap();
        frame_buffer.clear(Rgb565::RED).unwrap();

        let select = current_coordinates.sel_x_1 + current_coordinates.sel_y_1 * 3;
        let charset = CHARSETS[select as usize];
        if f32::abs(x_0 - current_coordinates.x_0) > 0.01
            || f32::abs(y_0 - current_coordinates.y_0) > 0.01
            || current_select != select
        {
            x_0 = current_coordinates.x_0;
            y_0 = current_coordinates.y_0;
            current_select = select;
            for y in (0..POSITION_PAD_DIAMETER).step_by(BAND_HEIGHT as usize) {
                let y_offset = y as i32;
                let height = BAND_HEIGHT.min((POSITION_PAD_DIAMETER - y) as i32) as u32;
                let mut cropped = frame_buffer.cropped(&Rectangle::new(Point::new(0, 0), Size::new(320, height)));
                let mut trans0 = cropped.translated(Point::new((POSITION_PAD_DIAMETER + 10) as i32, -y_offset));
                draw_position(&mut trans0, current_coordinates.x_0, current_coordinates.y_0, current_coordinates.sel_x_0, current_coordinates.sel_y_0, current_coordinates.pressed, charset);
                let mut trans1 = cropped.translated(Point::new(0, -y_offset));
                draw_position(&mut trans1, current_coordinates.x_1, current_coordinates.y_1, current_coordinates.sel_x_1, current_coordinates.sel_y_1, false, "");

                display
                    .draw(
                        Point::new(0, (BAND_HEIGHT + 1) * 4 + y_offset),
                        Size::new(320, height),
                        frame_buffer.data.iter().copied(),
                    )
                    .await
                    .unwrap();
                frame_buffer.clear(Rgb565::RED).unwrap();
            }
        }

        fill_and_draw_time(&mut frame_buffer, time, &mut buffer_time);
        draw_touch_buffer(
            &mut frame_buffer,
            &mut buffer_touch,
            BAND_HEIGHT * 2,
            Rgb565::WHITE,
        );
        display
            .draw(
                Point::new(0, (BAND_HEIGHT + 1) * 2),
                frame_buffer.size(),
                frame_buffer.data.iter().copied(),
            )
            .await
            .unwrap();
        frame_buffer.clear(Rgb565::RED).unwrap();

        let mut draw_band_3 = false;

        // Latest-value watch: redraw whenever the address changes, always showing
        // the most recent value rather than a stale queued one.
        if let Some(line) = address.try_changed() {
            let mut value = heapless::String::<22>::new();
            value
                .push_str(
                    line.split_at_checked(22)
                        .map(|(s, _)| s)
                        .unwrap_or(line.as_str()),
                )
                .unwrap();
            draw_band_3 = true;
            message = Some(value);
        }

        if min_v != current_coordinates.min_v || max_v != current_coordinates.max_v {
            min_v = current_coordinates.min_v;
            max_v = current_coordinates.max_v;
            draw_band_3 = true;
        }

        if draw_band_3 {
            draw_min_max(&mut frame_buffer, 'V', min_v, max_v, 0);

            if let Some(value) = &message {
                draw_text(&mut frame_buffer, value, 10 + 1 * 110, 1 + 6 + 11);
            }

            display
                .draw(
                    Point::new(0, (BAND_HEIGHT + 1) * 3),
                    frame_buffer.size(),
                    frame_buffer.data.iter().copied(),
                )
                .await
                .unwrap();
            frame_buffer.clear(Rgb565::RED).unwrap();
        }

        time = Some(start.elapsed().as_millis());
        Timer::after(Duration::from_millis(10)).await;
    }
}

fn draw_touch_buffer<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    buffer_touch: &mut Deque<TouchInputResponse, 100>,
    shift: i32,
    color: Rgb565,
) where
    <T as DrawTarget>::Error: Debug,
{
    let mut x_0 = -1;
    let mut y_0 = -1;
    for touch in buffer_touch.iter() {
        match touch {
            TouchInputResponse::Pressed { x, y } => {
                x_0 = *x;
                y_0 = *y;
            }
            TouchInputResponse::Moved { x, y } | TouchInputResponse::Released { x, y } => {
                if x_0 >= 0 && y_0 >= 0 {
                    Line::new(Point::new(x_0, y_0 - shift), Point::new(*x, *y - shift))
                        .into_styled(PrimitiveStyle::with_stroke(color, 1))
                        .draw(display)
                        .unwrap();
                }
                x_0 = *x;
                y_0 = *y;
            }
            _ => {}
        }
    }
}

fn draw_position<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    x: f32,
    y: f32,
    selector_x: i8,
    selector_y: i8,
    pressed: bool,
    charset: &str,
) where
    <T as DrawTarget>::Error: Debug,
{
    let y_0 = 0;
    let diameter = POSITION_PAD_DIAMETER as i32;
    let margin = (POSITION_PAD_DIAMETER as f32 * 0.05) as i32;
    let x_0 = 0;
    Rectangle::new(
        Point::new(x_0, y_0),
        Size::new(diameter as u32, diameter as u32),
    )
    .into_styled(
        PrimitiveStyleBuilder::new()
            .stroke_width(1)
            .fill_color(if charset.len() > 0 {
                Rgb565::CSS_LIGHT_SALMON
            } else {
                Rgb565::CSS_ORANGE_RED
            })
            .stroke_color(Rgb565::BLACK)
            .build(),
    )
    .draw(display)
    .unwrap();
    Rectangle::new(
        Point::new(x_0 + margin, y_0 + margin),
        Size::new(
            (diameter - margin * 2) as u32,
            (diameter - margin * 2) as u32,
        ),
    )
    .into_styled(
        PrimitiveStyleBuilder::new()
            .fill_color(Rgb565::CSS_ORANGE_RED)
            .build(),
    )
    .draw(display)
    .unwrap();

    let border = if charset.len() > 0 && pressed { 3 } else { 1 };
    Rectangle::new(
        Point::new(
            x_0 + diameter / 3 * selector_x as i32,
            y_0 + diameter / 3 * selector_y as i32,
        ),
        Size::new(diameter as u32 / 3, diameter as u32 / 3),
    )
    .into_styled(
        PrimitiveStyleBuilder::new()
            .stroke_width(border)
            .stroke_color(Rgb565::CSS_ORANGE)
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
    if charset.len() == 0 {
        for x in 0..3 {
            for y in 0..3 {
                let x_pos = x_0 + diameter / 3 * x + 7;
                let y_pos = y_0 + diameter / 3 * y + 12;

                for (index, ch) in CHARSETS[(x + 3 * y) as usize].chars().enumerate() {
                    let index = if index < 4 {
                        index as i32
                    } else {
                        index as i32 + 1
                    };
                    if let Err(_) = core::fmt::write(&mut char, format_args!("{}", ch)) {
                        warn!("Failed to write char: {}", ch);
                        char.clear();
                        continue;
                    }
                    Text::new(
                        &char,
                        Point::new(x_pos + 10 * (index % 3), y_pos + 10 * (index / 3)),
                        MonoTextStyle::new(&FONT_6X10, Rgb565::BLACK),
                    )
                    .draw(display)
                    .unwrap();
                    char.clear();
                }
            }
        }
    }
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
}

fn draw_axis_min_max<T: DrawTarget<Color = Rgb565>, M: core::fmt::Display>(
    display: &mut T,
    min: M,
    max: M,
) where
    <T as DrawTarget>::Error: Debug,
{
    // 8x13 font numbers are 9 pixels high, text is drawn up from the point.
    let mut value = heapless::String::<22>::new();
    let Ok(_) = core::fmt::write(&mut value, format_args!("{}", max)) else {
        info!("Failed to write max");
        return;
    };
    Text::new(
        &value,
        Point::new(1, 10),
        MonoTextStyle::new(&FONT_8X13_BOLD, Rgb565::BLACK),
    )
    .draw(display)
    .unwrap();

    value.clear();
    let Ok(_) = core::fmt::write(&mut value, format_args!("{}", min)) else {
        info!("Failed to write min");
        return;
    };
    Text::new(
        &value,
        Point::new(1, BAND_HEIGHT - 2),
        MonoTextStyle::new(&FONT_8X13_BOLD, Rgb565::BLACK),
    )
    .draw(display)
    .unwrap();
}

fn draw_min_max<M: core::fmt::Display, T: DrawTarget<Color = Rgb565>>(
    target: &mut T,
    prefix: char,
    min: M,
    max: M,
    band: i32,
) where
    <T as DrawTarget>::Error: Debug,
{
    let mut value = heapless::String::<22>::new();
    if core::fmt::write(&mut value, format_args!("{}={:4}, {:4}", prefix, min, max)).is_err() {
        info!("Failed to write min and max");
        return;
    };
    let x_0 = 10 + band * 110;
    let y_0 = 1 + 6 + 11;
    draw_text(target, &value, x_0, y_0);
}

fn draw_text<T: DrawTarget<Color = Rgb565>>(target: &mut T, value: &str, x: i32, y: i32)
where
    <T as DrawTarget>::Error: Debug,
{
    Text::new(
        value,
        Point::new(x, y),
        MonoTextStyle::new(&FONT_8X13_ITALIC, Rgb565::BLACK),
    )
    .draw(target)
    .unwrap();
}

#[allow(dead_code)]
fn redraw_and_fill<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    color: Rgb565,
    value: i32,
    buffer: &mut Deque<i32, 321>,
) where
    <T as DrawTarget>::Error: Debug,
{
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

fn draw_buffer<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    buffer: &Deque<i32, 321>,
    color: Rgb565,
) where
    <T as DrawTarget>::Error: Debug,
{
    let mut flat_x = -1;
    let mut flat_y = -1;
    let mut skipped = false;
    for (x0, (y0, y1)) in buffer.iter().zip(buffer.iter().skip(1)).enumerate() {
        if flat_y == *y1 {
            skipped = true;
            continue;
        } else {
            if flat_x != -1 {
                Line::new(Point::new(flat_x, flat_y), Point::new(x0 as i32, flat_y))
                    .into_styled(PrimitiveStyle::with_stroke(color, 1))
                    .draw(display)
                    .unwrap();
            }
            flat_x = (x0 + 1) as i32;
            flat_y = *y1;
            Line::new(Point::new(x0 as i32, *y0), Point::new(x0 as i32 + 1, *y1))
                .into_styled(PrimitiveStyle::with_stroke(color, 1))
                .draw(display)
                .unwrap();
            skipped = false;
        }
    }
    if skipped {
        Line::new(
            Point::new(flat_x, flat_y),
            Point::new(buffer.len() as i32, flat_y),
        )
        .into_styled(PrimitiveStyle::with_stroke(color, 1))
        .draw(display)
        .unwrap();
    }
}

/// Draw the time graph scaling it to currently visible min and max, this one is about 5 ms slower.
///
/// ```no_run
/// let mut buffer_time: Deque<u64, 321> = Deque::new();
///
/// draw_time(&mut self.display, start, &mut buffer_time);
/// ```
#[allow(dead_code)]
fn fill_and_draw_time<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    elapsed: Option<u64>,
    buffer_time: &mut Deque<u64, 321>,
) where
    <T as DrawTarget>::Error: Debug,
{
    let Some(elapsed_ms) = elapsed else {
        return;
    };
    if let Some((min, max)) = draw_buffer_scaled(display, &buffer_time, Rgb565::WHITE) {
        draw_axis_min_max(display, min, max);
    }

    buffer_time.push_back(elapsed_ms).unwrap();
    if buffer_time.len() > 320 {
        buffer_time.pop_front();
    }
}

fn draw_buffer_scaled<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    input_buffer: &Deque<u64, 321>,
    color: Rgb565,
) -> Option<(u64, u64)>
where
    <T as DrawTarget>::Error: Debug,
{
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
        let value = pos_to_y!(value_to_percent!(*raw_value, min, max, false));
        Pixel(Point::new(index as i32, value), color)
            .draw(display)
            .unwrap();
    }
    Some((min, max))
}

#[allow(dead_code)]
async fn rainbow_text<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    text: &Option<heapless::String<MESSAGE_SIZE>>,
) where
    <T as DrawTarget>::Error: Debug,
{
    let Some(text) = text else {
        // Park forever without arming a far-future hardware timer (see buzzer.rs).
        core::future::pending::<()>().await;
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
pub async fn test_display<T: DrawTarget<Color = Rgb565>>(display: &mut T)
where
    <T as DrawTarget>::Error: Debug,
{
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
