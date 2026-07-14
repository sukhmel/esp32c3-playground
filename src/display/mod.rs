#[cfg(not(feature = "async_ili9341"))]
pub mod ili9341;
#[cfg(feature = "async_ili9341")]
pub mod ili9341_async;

use crate::input::{
    CH_BACKSPACE, CH_DELETE, CH_DOWN_ARROW, CH_ENTER, CH_ESCAPE, CH_LEFT_ARROW, CH_RIGHT_ARROW,
    CH_TAB, CH_UP_ARROW, CHARSETS, value_to_percent,
};
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
/// Redraw the coordinate and timing graphs after this many readings.
const GRAPH_REDRAW_VALUE_INTERVAL: usize = 100;
/// Redraw the coordinate and timing graphs after this much time to reduce draw count.
const GRAPH_REDRAW_INTERVAL_MS: u64 = 1_000;
const TIME_AXIS_GLYPH_WIDTH: u32 = 8;
static POSITION_PAD_DIAMETER: usize = 240usize
    .checked_sub(4 * (BAND_HEIGHT + 1) as usize)
    .unwrap();

/// Two pads side by side with a 10 px gap; columns right of this never change.
static POSITION_PAD_AREA_WIDTH: u32 = POSITION_PAD_DIAMETER as u32 * 2 + 10;
const POSITION_PAD_ROWS: usize = 3;
const POSITION_PAD_ROW_MAX_HEIGHT: usize = POSITION_PAD_DIAMETER.div_ceil(POSITION_PAD_ROWS);
const POSITION_PAD_FRAME_BUFFER_LEN: usize =
    POSITION_PAD_AREA_WIDTH as usize * POSITION_PAD_ROW_MAX_HEIGHT;

#[derive(Clone, Copy)]
struct PositionPadState {
    dot_0: Point,
    dot_1: Point,
    selector_x_0: i8,
    selector_y_0: i8,
    selector_x_1: i8,
    selector_y_1: i8,
    pressed: bool,
    charset: usize,
}

impl PositionPadState {
    fn from_reading(reading: &Reading, charset: usize) -> Self {
        let diameter = POSITION_PAD_DIAMETER as f32;
        Self {
            dot_0: Point::new(
                (reading.x_0 * diameter) as i32,
                ((1.0 - reading.y_0) * diameter) as i32,
            ),
            dot_1: Point::new(
                (reading.x_1 * diameter) as i32,
                ((1.0 - reading.y_1) * diameter) as i32,
            ),
            selector_x_0: reading.sel_x_0,
            selector_y_0: reading.sel_y_0,
            selector_x_1: reading.sel_x_1,
            selector_y_1: reading.sel_y_1,
            pressed: reading.pressed,
            charset,
        }
    }
}

fn position_pad_row_changed(
    previous: Option<PositionPadState>,
    current: PositionPadState,
    row: Rectangle,
) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    if previous.charset != current.charset {
        return true;
    }

    let row_y0 = row.top_left.y;
    let row_y1 = row_y0 + row.size.height as i32;
    let intersects = |top: i32, bottom: i32| bottom > row_y0 && top < row_y1;
    let dot_changed_in_row = |old: Point, new: Point| {
        old != new && (intersects(old.y - 1, old.y + 6) || intersects(new.y - 1, new.y + 6))
    };
    if dot_changed_in_row(previous.dot_0, current.dot_0)
        || dot_changed_in_row(previous.dot_1, current.dot_1)
    {
        return true;
    }

    let diameter = POSITION_PAD_DIAMETER as i32;
    let selector_intersects = |selector_y: i8, border: i32| {
        let top = diameter / 3 * selector_y as i32;
        intersects(top - border, top + diameter / 3 + border)
    };
    let selector_0_changed = previous.selector_x_0 != current.selector_x_0
        || previous.selector_y_0 != current.selector_y_0
        || previous.pressed != current.pressed;
    if selector_0_changed
        && (selector_intersects(previous.selector_y_0, if previous.pressed { 3 } else { 1 })
            || selector_intersects(current.selector_y_0, if current.pressed { 3 } else { 1 }))
    {
        return true;
    }

    let selector_1_changed = previous.selector_x_1 != current.selector_x_1
        || previous.selector_y_1 != current.selector_y_1;
    selector_1_changed
        && (selector_intersects(previous.selector_y_1, 1)
            || selector_intersects(current.selector_y_1, 1))
}

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
    frame_buffer.clear(Rgb565::RED).unwrap();
    let mut time = None;
    let mut min_v = u16::MAX;
    let mut max_v = u16::MIN;
    let mut current_coordinates = Reading::default();
    let mut previous_position_pad_state = None;
    let mut values_since_graph_redraw = 0usize;
    let mut last_graph_redraw = Instant::now();
    let mut displayed_time_range = None;
    loop {
        let start = Instant::now();
        let mut drew_anything = false;
        let mut loaded = 0;
        let mut coordinate_values_loaded = 0;
        while let Ok(coordinates) = channel.try_receive() {
            loaded += 1;
            coordinate_values_loaded += 1;
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
        values_since_graph_redraw =
            values_since_graph_redraw.saturating_add(coordinate_values_loaded);
        let graph_redraw_due = values_since_graph_redraw >= GRAPH_REDRAW_VALUE_INTERVAL
            || last_graph_redraw.elapsed().as_millis() >= GRAPH_REDRAW_INTERVAL_MS;
        if loaded == 0 && !graph_redraw_due {
            Timer::after(Duration::from_millis(100)).await;
            continue;
        }

        let mut time_range_changed = false;
        if let Some(elapsed_ms) = time.take() {
            buffer_time.push_back(elapsed_ms).unwrap();
            if buffer_time.len() > 320 {
                buffer_time.pop_front();
            }
            time_range_changed = time_range(&buffer_time) != displayed_time_range;
        }

        if graph_redraw_due {
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
            drew_anything = true;
            frame_buffer.clear(Rgb565::RED).unwrap();
        }

        let select = current_coordinates.sel_x_1 + current_coordinates.sel_y_1 * 3;
        let charset = CHARSETS[select as usize];
        let position_pad_state =
            PositionPadState::from_reading(&current_coordinates, select as usize);
        drop(frame_buffer);
        {
            let position_frame_buffer_data = frame_buffer_data
                .first_chunk_mut::<POSITION_PAD_FRAME_BUFFER_LEN>()
                .unwrap();
            let mut position_frame_buffer = FrameBuf::new(
                position_frame_buffer_data,
                POSITION_PAD_AREA_WIDTH as usize,
                POSITION_PAD_ROW_MAX_HEIGHT,
            );
            for row in 0..POSITION_PAD_ROWS {
                let y_offset = POSITION_PAD_DIAMETER * row / POSITION_PAD_ROWS;
                let y_end = POSITION_PAD_DIAMETER * (row + 1) / POSITION_PAD_ROWS;
                let height = (y_end - y_offset) as u32;
                let band = Rectangle::new(
                    Point::new(0, y_offset as i32),
                    Size::new(POSITION_PAD_DIAMETER as u32, height),
                );
                if !position_pad_row_changed(previous_position_pad_state, position_pad_state, band)
                {
                    continue;
                }

                position_frame_buffer.clear(Rgb565::RED).unwrap();
                let mut cropped = position_frame_buffer.cropped(&Rectangle::new(
                    Point::new(0, 0),
                    Size::new(POSITION_PAD_AREA_WIDTH, height),
                ));
                let mut trans0 = cropped.translated(Point::new(
                    (POSITION_PAD_DIAMETER + 10) as i32,
                    -(y_offset as i32),
                ));
                draw_position(
                    &mut trans0,
                    band,
                    current_coordinates.x_0,
                    current_coordinates.y_0,
                    current_coordinates.sel_x_0,
                    current_coordinates.sel_y_0,
                    current_coordinates.pressed,
                    charset,
                );
                let mut trans1 = cropped.translated(Point::new(0, -(y_offset as i32)));
                draw_position(
                    &mut trans1,
                    band,
                    current_coordinates.x_1,
                    current_coordinates.y_1,
                    current_coordinates.sel_x_1,
                    current_coordinates.sel_y_1,
                    false,
                    "",
                );

                display
                    .draw(
                        Point::new(0, (BAND_HEIGHT + 1) * 4 + y_offset as i32),
                        Size::new(POSITION_PAD_AREA_WIDTH, height),
                        position_frame_buffer
                            .data
                            .iter()
                            .take(POSITION_PAD_AREA_WIDTH as usize * height as usize)
                            .copied(),
                    )
                    .await
                    .unwrap();
                drew_anything = true;
            }
        }
        previous_position_pad_state = Some(position_pad_state);
        frame_buffer = FrameBuf::new(&mut frame_buffer_data, 320, BAND_HEIGHT as usize);
        frame_buffer.clear(Rgb565::RED).unwrap();

        if graph_redraw_due {
            draw_time(&mut frame_buffer, &buffer_time);
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
            drew_anything = true;
            frame_buffer.clear(Rgb565::RED).unwrap();
            values_since_graph_redraw = 0;
            last_graph_redraw = Instant::now();
            displayed_time_range = time_range(&buffer_time);
        } else if time_range_changed {
            let Some((min, max)) = time_range(&buffer_time) else {
                unreachable!();
            };
            let strip_width = displayed_time_range
                .map(|(old_min, old_max)| time_axis_width(old_min, old_max))
                .unwrap_or(0)
                .max(time_axis_width(min, max));
            let mut strip = frame_buffer.cropped(&Rectangle::new(
                Point::new(0, 0),
                Size::new(strip_width, BAND_HEIGHT as u32),
            ));
            draw_axis_min_max(&mut strip, min, max);
            display
                .draw(
                    Point::new(0, (BAND_HEIGHT + 1) * 2),
                    Size::new(strip_width, BAND_HEIGHT as u32),
                    frame_buffer.data.chunks(320).flat_map(|row| {
                        row[..strip_width as usize].iter().copied()
                    }),
                )
                .await
                .unwrap();
            drew_anything = true;
            frame_buffer.clear(Rgb565::RED).unwrap();
            displayed_time_range = Some((min, max));
        }

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
            drew_anything = true;
            frame_buffer.clear(Rgb565::RED).unwrap();
        }

        time = drew_anything.then(|| start.elapsed().as_millis());
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

/// Draw the part of a position pad that intersects the frame buffer future draw location.
#[allow(clippy::too_many_arguments)]
fn draw_position<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    band: Rectangle,
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
    let band_y0 = band.top_left.y;
    let band_y1 = band_y0 + band.size.height as i32;
    // Does the half-open row range [top, bottom) intersect the band?
    let rows_visible = |top: i32, bottom: i32| bottom > band_y0 && top < band_y1;

    Rectangle::new(
        Point::new(x_0, y_0),
        Size::new(diameter as u32, diameter as u32),
    )
    .intersection(&band)
    .into_styled(
        PrimitiveStyleBuilder::new()
            .fill_color(if charset.len() > 0 {
                Rgb565::CSS_LIGHT_SALMON
            } else {
                Rgb565::CSS_ORANGE_RED
            })
            .build(),
    )
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
    Rectangle::new(
        Point::new(x_0 + margin, y_0 + margin),
        Size::new(
            (diameter - margin * 2) as u32,
            (diameter - margin * 2) as u32,
        ),
    )
    .intersection(&band)
    .into_styled(
        PrimitiveStyleBuilder::new()
            .fill_color(Rgb565::CSS_ORANGE_RED)
            .build(),
    )
    .draw(display)
    .unwrap();

    let border: i32 = if charset.len() > 0 && pressed { 3 } else { 1 };
    let selector_top = y_0 + diameter / 3 * selector_y as i32;
    if rows_visible(selector_top - border, selector_top + diameter / 3 + border) {
        Rectangle::new(
            Point::new(x_0 + diameter / 3 * selector_x as i32, selector_top),
            Size::new(diameter as u32 / 3, diameter as u32 / 3),
        )
        .into_styled(
            PrimitiveStyleBuilder::new()
                .stroke_width(border as u32)
                .stroke_color(Rgb565::CSS_ORANGE)
                .build(),
        )
        .draw(display)
        .unwrap();
    }
    let dot_top = y_0 + ((1.0 - y) * diameter as f32) as i32;
    if rows_visible(dot_top - 1, dot_top + 6) {
        Circle::new(Point::new(x_0 + (x * diameter as f32) as i32, dot_top), 4)
            .into_styled(
                PrimitiveStyleBuilder::new()
                    .stroke_width(1)
                    .stroke_color(Rgb565::BLACK)
                    .fill_color(Rgb565::WHITE)
                    .build(),
            )
            .draw(display)
            .unwrap();
    }
    let mut char = heapless::String::<4>::new();
    if charset.len() == 0 {
        for x in 0..3 {
            for y in 0..3 {
                let x_pos = x_0 + diameter / 3 * x + 7;
                let y_pos = y_0 + diameter / 3 * y + 12;
                // The cell's three text rows span roughly [y_pos - 10, y_pos + 30).
                if !rows_visible(y_pos - 10, y_pos + 30) {
                    continue;
                }

                for (index, ch) in CHARSETS[(x + 3 * y) as usize].chars().enumerate() {
                    let index = index as i32;
                    let text_y = y_pos + 10 * (index / 3);
                    if !rows_visible(text_y - 10, text_y + 4) {
                        continue;
                    }
                    if draw_special_glyph(
                        display,
                        ch,
                        Point::new(x_pos + 10 * (index % 3), text_y),
                        6,
                        8,
                    ) {
                        continue;
                    }
                    if let Err(_) = core::fmt::write(&mut char, format_args!("{}", ch)) {
                        warn!("Failed to write char: {}", ch);
                        char.clear();
                        continue;
                    }
                    Text::new(
                        &char,
                        Point::new(x_pos + 10 * (index % 3), text_y),
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
        let index = index as i32;
        let x_pos = x_0 + diameter / 3 * (index % 3) + diameter / 6 - 5;
        let y_pos = y_0 + diameter / 3 * (index / 3) + diameter / 6 + 4;
        if !rows_visible(y_pos - 20, y_pos + 6) {
            continue;
        }
        if draw_special_glyph(display, ch, Point::new(x_pos, y_pos), 11, 15) {
            continue;
        }
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

// Some arrows still look off, but I'm tired of fine-tuning them.
// Maybe I should learn to make fonts like the ones in `embedded_graphics`.
fn draw_special_glyph<T: DrawTarget<Color = Rgb565>>(
    display: &mut T,
    ch: char,
    anchor: Point,
    w: i32,
    h: i32,
) -> bool {
    let stroke_width = if w > 8 { 2 } else { 1 };
    let style = PrimitiveStyle::with_stroke(Rgb565::BLACK, stroke_width);
    let left = anchor.x;
    let right = anchor.x + w - 1;
    let bottom = anchor.y;
    let top = anchor.y - h + 2;
    let center_x = (left + right) / 2;
    let center_y = (top + bottom) / 2;
    let hy = h / 4; // arrowhead y extent
    let hx = w / 3; // arrowhead x extent
    // upwards arrowhead y extent is larger to account for stroke width
    // right to left line is wide downwards, left to right is wide upwards
    // for the same reason the left arrowhead line is also shifted
    let stroke_adjustment = stroke_width as i32 - 1;
    let mut line = |a: Point, b: Point| {
        let _ = Line::new(a, b).into_styled(style).draw(display);
    };
    match ch {
        CH_BACKSPACE => {
            // ← left arrow with a stop
            line(Point::new(left, center_y), Point::new(right, center_y));
            line(
                Point::new(left + stroke_adjustment * 2, center_y - stroke_adjustment),
                Point::new(
                    left + stroke_width as i32 * hx - stroke_adjustment,
                    center_y - hy - stroke_adjustment,
                ),
            );
            line(
                Point::new(left + stroke_adjustment * 2, center_y + stroke_adjustment),
                Point::new(left + stroke_width as i32 * hx - stroke_adjustment, center_y + hy + stroke_adjustment),
            );
            line(
                Point::new(left, center_y - hy - stroke_adjustment),
                Point::new(left, center_y + hy + stroke_adjustment),
            );
        }
        // not fine-tuned
        CH_DELETE => {
            // → right arrow with a stop
            line(Point::new(left, center_y), Point::new(right, center_y));
            line(
                Point::new(right, center_y - stroke_adjustment),
                Point::new(right - hx, center_y - hy - stroke_adjustment),
            );
            line(
                Point::new(right, center_y),
                Point::new(right - hx, center_y + hy),
            );
            line(
                Point::new(right, center_y - hy - stroke_adjustment),
                Point::new(right, center_y + hy),
            );
        }
        // fine-tuned
        CH_ESCAPE => {
            // ↑ up arrow from box instead of ⎋ for now
            let box_right = right - 1;
            line(Point::new(center_x, bottom - hy), Point::new(center_x, top));
            line(
                Point::new(center_x - stroke_adjustment, top),
                Point::new(center_x - hx - stroke_adjustment, top + hy),
            );
            line(
                Point::new(center_x - stroke_adjustment, top),
                Point::new(center_x + hx - stroke_adjustment, top + hy),
            );
            line(Point::new(left, center_y + 2), Point::new(left, bottom));
            line(Point::new(left, bottom), Point::new(box_right, bottom));
            line(Point::new(box_right, center_y + 2), Point::new(box_right, bottom));
        }
        // fine-tuned
        CH_ENTER => {
            // ↵ return: down the right edge, left along the middle, arrowhead left
            let shift = hy - 1 + stroke_adjustment;
            line(Point::new(right, top + shift), Point::new(right, center_y + shift));
            line(Point::new(left, center_y + shift), Point::new(right, center_y + shift));
            line(
                Point::new(left, center_y + shift),
                Point::new(left + hx + stroke_adjustment, center_y - hy + shift - stroke_adjustment),
            );
            line(
                Point::new(left, center_y + shift),
                Point::new(left + hx + stroke_adjustment, center_y + hy + shift + stroke_adjustment),
            );
        }
        // fine-tuned
        CH_TAB => {
            // ↹ two arrows opposite way
            let y_arrow = top + h / 4 - 1 + stroke_adjustment;
            line(Point::new(left, y_arrow), Point::new(right, y_arrow));
            line(
                Point::new(left, y_arrow),
                Point::new(left + hx, y_arrow - hy),
            );
            line(
                Point::new(left, y_arrow),
                Point::new(left + hx, y_arrow + hy),
            );

            let y_arrow = top + 3 * h / 4;
            line(Point::new(left, y_arrow), Point::new(right, y_arrow));
            line(
                Point::new(right, y_arrow - stroke_adjustment),
                Point::new(right - hx, y_arrow - hy - stroke_adjustment),
            );
            line(
                Point::new(right, y_arrow - stroke_adjustment),
                Point::new(right - hx, y_arrow + hy - stroke_adjustment),
            );
        }
        // fine-tuned
        CH_DOWN_ARROW => {
            // ↓
            line(Point::new(center_x, bottom), Point::new(center_x, top));
            line(
                Point::new(center_x - stroke_adjustment, bottom - stroke_adjustment),
                Point::new(
                    center_x - hx - stroke_adjustment,
                    bottom - hy - stroke_adjustment,
                ),
            );
            line(
                Point::new(center_x + stroke_adjustment, bottom - stroke_adjustment),
                Point::new(center_x + hx + stroke_adjustment, bottom - hy - stroke_adjustment),
            );
        }
        // fine-tuned
        CH_UP_ARROW => {
            // ↑
            line(Point::new(center_x, bottom), Point::new(center_x, top));
            line(
                Point::new(
                    center_x - hx - stroke_adjustment,
                    top + hy + stroke_adjustment,
                ),
                Point::new(center_x - stroke_adjustment, top + stroke_adjustment),
            );
            line(
                Point::new(center_x + hx + stroke_adjustment, top + hy + stroke_adjustment),
                Point::new(center_x + stroke_adjustment, top + stroke_adjustment),
            );
        }
        // fine-tuned
        CH_LEFT_ARROW => {
            // ←
            line(Point::new(left, center_y), Point::new(right, center_y));
            line(
                Point::new(left + hx + stroke_adjustment, center_y - hy - stroke_adjustment * 2),
                Point::new(left, center_y - stroke_adjustment),
            );
            line(
                Point::new(left + hx + stroke_adjustment, center_y + hy),
                Point::new(left, center_y - stroke_adjustment),
            );
        }
        // fine-tuned
        CH_RIGHT_ARROW => {
            // →
            line(Point::new(left, center_y), Point::new(right, center_y));
            line(
                Point::new(right, center_y - stroke_adjustment),
                Point::new(right - hx - stroke_adjustment, center_y - hy - stroke_adjustment * 2),
            );
            line(
                Point::new(right, center_y - stroke_adjustment),
                Point::new(right - hx - stroke_adjustment, center_y + hy),
            );
        }
        _ => return false,
    }
    true
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

fn time_range(buffer: &Deque<u64, 321>) -> Option<(u64, u64)> {
    let mut values = buffer.iter();
    let first = *values.next()?;
    let mut min = first;
    let mut max = first;
    for value in values {
        min = min.min(*value);
        max = max.max(*value);
    }
    Some((min, max))
}

fn time_axis_width(min: u64, max: u64) -> u32 {
    fn decimal_digits(mut value: u64) -> u32 {
        let mut digits = 1;
        while value >= 10 {
            value /= 10;
            digits += 1;
        }
        digits
    }

    1 + TIME_AXIS_GLYPH_WIDTH * decimal_digits(min).max(decimal_digits(max))
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

/// Draw the time graph scaling it to currently visible min and max.
///
/// ```no_run
/// let mut buffer_time: Deque<u64, 321> = Deque::new();
///
/// draw_time(&mut self.display, &buffer_time);
/// ```
#[allow(dead_code)]
fn draw_time<T: DrawTarget<Color = Rgb565>>(display: &mut T, buffer_time: &Deque<u64, 321>)
where
    <T as DrawTarget>::Error: Debug,
{
    if let Some((min, max)) = draw_buffer_scaled(display, &buffer_time, Rgb565::WHITE) {
        draw_axis_min_max(display, min, max);
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
