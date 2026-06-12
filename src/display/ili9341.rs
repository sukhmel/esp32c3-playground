use crate::input::{CHARSETS, value_to_percent};
use crate::inter_task::{CoordinatesReceiver, MESSAGE_SIZE, MessageReceiver};
use crate::rainbow::{RAINBOW_RGB565_128, RAINBOW_RGB565_256, rgb565_rainbow};
use ariel_os::debug::log::{info, warn};
use ariel_os::time::{Instant, Timer};
use ariel_os_hal::gpio::Output;
use core::fmt::Debug;
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
        iso_8859_5::{FONT_8X13_ITALIC, FONT_8X13_BOLD, FONT_9X18, FONT_9X18_BOLD, FONT_10X20},
    },
    prelude::*,
    text::Text,
};
use embedded_graphics_framebuf::FrameBuf;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::spi::master::Spi;
use heapless::Deque;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorOrder, Rotation};
use mipidsi::{Builder, Display as DisplayImpl, models::ILI9341Rgb565, options::Orientation};

static INPUT_COLORS: [Rgb565; 4] = [
    Rgb565::BLUE,
    Rgb565::CSS_VIOLET,
    Rgb565::YELLOW,
    Rgb565::CSS_DARK_GREEN,
];
static BAND_HEIGHT: i32 = 30;
static POSITION_PAD_DIAMETER: usize = 240usize.checked_sub(4 * BAND_HEIGHT as usize).unwrap();

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

    pub async fn debug_input(&mut self, channel: CoordinatesReceiver, address: MessageReceiver) {
        let mut message = None;
        let mut position_frame_buffer_data =
            [Rgb565::CSS_ORANGE_RED; POSITION_PAD_DIAMETER * POSITION_PAD_DIAMETER];
        let mut position_frame_buffer = FrameBuf::new(
            &mut position_frame_buffer_data,
            POSITION_PAD_DIAMETER,
            POSITION_PAD_DIAMETER,
        );
        let mut frame_buffer_data = [Rgb565::RED; (320 * BAND_HEIGHT) as usize];
        let mut frame_buffer = FrameBuf::new(&mut frame_buffer_data, 320, BAND_HEIGHT as usize);
        let mut buffer_x0: Deque<i32, 350> = Deque::new();
        let mut buffer_y0: Deque<i32, 350> = Deque::new();
        let mut buffer_x1: Deque<i32, 350> = Deque::new();
        let mut buffer_y1: Deque<i32, 350> = Deque::new();
        let mut buffer_time: Deque<u64, 350> = Deque::new();
        self.display.clear(Rgb565::RED).unwrap();
        for band in 1..=3 {
            Line::new(
                Point::new(0, (BAND_HEIGHT + 1) * band - 1),
                Point::new(320, (BAND_HEIGHT + 1) * band - 1),
            )
            .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLACK, 1))
            .draw(&mut self.display)
            .unwrap();
        }
        let mut time = None;
        let mut min_v = u16::MAX;
        let mut max_v = u16::MIN;
        loop {
            let coordinates = channel.receive().await;
            let start = Instant::now();
            let x0 = coordinates.x_0;
            let y0 = coordinates.y_0;
            let x1 = coordinates.x_1;
            let y1 = coordinates.y_1;

            redraw_and_fill(
                &mut frame_buffer,
                INPUT_COLORS[0],
                pos_to_y!(x0),
                &mut buffer_x0,
            );
            redraw_and_fill(
                &mut frame_buffer,
                INPUT_COLORS[1],
                pos_to_y!(y0),
                &mut buffer_y0,
            );
            self.display
                .fill_contiguous(
                    &Rectangle::new(Point::new(0, BAND_HEIGHT + 1), frame_buffer.size()),
                    frame_buffer.data.iter().copied(),
                )
                .unwrap();
            frame_buffer.clear(Rgb565::RED).unwrap();
            redraw_and_fill(
                &mut frame_buffer,
                INPUT_COLORS[2],
                pos_to_y!(x1),
                &mut buffer_x1,
            );
            redraw_and_fill(
                &mut frame_buffer,
                INPUT_COLORS[3],
                pos_to_y!(y1),
                &mut buffer_y1,
            );
            self.display
                .fill_contiguous(
                    &Rectangle::new(Point::new(0, 0), frame_buffer.size()),
                    frame_buffer.data.iter().copied(),
                )
                .unwrap();
            frame_buffer.clear(Rgb565::RED).unwrap();
            if min_v != coordinates.min_v || max_v != coordinates.max_v {
                min_v = coordinates.min_v;
                max_v = coordinates.max_v;
                draw_min_max(
                    &mut self.display,
                    'V',
                    min_v,
                    max_v,
                    0,
                );
            }

            let select = coordinates.sel_x_1 + coordinates.sel_y_1 * 3;
            let charset = CHARSETS[select as usize];
            draw_position(
                &mut position_frame_buffer,
                x0,
                y0,
                coordinates.sel_x_0,
                coordinates.sel_y_0,
                coordinates.pressed,
                charset,
            );
            self.display
                .fill_contiguous(
                    &Rectangle::new(
                        Point::new((POSITION_PAD_DIAMETER + 10) as i32, (BAND_HEIGHT + 1) * 4),
                        position_frame_buffer.size(),
                    ),
                    position_frame_buffer.data.iter().copied(),
                )
                .unwrap();
            position_frame_buffer.clear(Rgb565::RED).unwrap();

            draw_position(
                &mut position_frame_buffer,
                x1,
                y1,
                coordinates.sel_x_1,
                coordinates.sel_y_1,
                false,
                "",
            );
            self.display
                .fill_contiguous(
                    &Rectangle::new(
                        Point::new(0, (BAND_HEIGHT + 1) * 4),
                        position_frame_buffer.size(),
                    ),
                    position_frame_buffer.data.iter().copied(),
                )
                .unwrap();
            position_frame_buffer.clear(Rgb565::CSS_ORANGE_RED).unwrap();

            fill_and_draw_time(&mut frame_buffer, time, &mut buffer_time);
            self.display
                .fill_contiguous(
                    &Rectangle::new(Point::new(0, (BAND_HEIGHT + 1) * 2), frame_buffer.size()),
                    frame_buffer.data.iter().copied(),
                )
                .unwrap();
            frame_buffer.clear(Rgb565::RED).unwrap();

            if message.is_none() && let Ok(line) = address.try_peek() {
                let mut value = heapless::String::<22>::new();
                value.push_str(line.split_at_checked(22).map(|(s, _)| s).unwrap_or(line.as_str())).unwrap();
                draw_text(&mut self.display, &value, 1);
                message = Some(value);
            }
            time = Some(start.elapsed().as_millis());
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
            .fill_color(Rgb565::CSS_LIGHT_SALMON)
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
    .into_styled(PrimitiveStyleBuilder::new().fill_color(Rgb565::CSS_ORANGE_RED).build())
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
) where <T as DrawTarget>::Error: Debug {
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

fn draw_min_max<T: core::fmt::Display>(
    display: &mut DisplayAlias<'_, '_>,
    prefix: char,
    min: T,
    max: T,
    band: i32,
) {
    let mut value = heapless::String::<22>::new();
    let Ok(_) = core::fmt::write(&mut value, format_args!("{}={:4}, {:4}", prefix, min, max)) else {
        info!("Failed to write min and max");
        return;
    };
    draw_text(display, &value, band);
}

fn draw_text(
    display: &mut DisplayAlias<'_, '_>,
    value: &heapless::String<22>,
    band: i32,
) {
    let y_0 = (BAND_HEIGHT + 1) * 3 + 1;
    let x_0 = 10 + band * 110;
    Rectangle::new(Point::new(x_0, y_0 + 6), Size::new(100, 13))
        .into_styled(PrimitiveStyleBuilder::new().fill_color(Rgb565::RED).build())
        .draw(display)
        .unwrap();
    Text::new(
        value,
        Point::new(x_0, y_0 + 15),
        MonoTextStyle::new(&FONT_8X13_ITALIC, Rgb565::BLACK),
    )
    .draw(display)
    .unwrap();
}

fn redraw_and_fill<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    color: Rgb565,
    value: i32,
    buffer: &mut Deque<i32, 350>,
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
    buffer: &Deque<i32, 350>,
    color: Rgb565,
) where
    <T as DrawTarget>::Error: Debug,
{
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
        draw_min_max(display, 't', *min_time, *max_time, 0);
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
fn fill_and_draw_time<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    elapsed: Option<u64>,
    buffer_time: &mut Deque<u64, 350>,
)
where
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
    input_buffer: &Deque<u64, 350>,
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
