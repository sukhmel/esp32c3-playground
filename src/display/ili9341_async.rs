extern crate alloc;

use crate::display::{DisplayTarget, debug_input};
use crate::inter_task::{CoordinatesReceiver, MessageReceiver, TouchReceiver};
use alloc::vec;
use ariel_os::time::Timer;
use ariel_os_hal::gpio::Output;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDeviceWithConfig;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_graphics::Drawable;
use embedded_graphics::geometry::Point;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use esp_hal::Async;
use esp_hal::delay::Delay;
use esp_hal::spi::master::{Config, Spi};
use esp_hal::time::Rate;
use sky_ili9341::{AsyncBuilder, AsyncDisplay, AsyncSpiInterface, ColorOrder, Orientation};

type Ili9341Display<'a, 'd> = AsyncDisplay<
    AsyncSpiInterface<SpiDeviceWithConfig<'a, NoopRawMutex, Spi<'d, Async>, Output>, Output>,
>;

pub struct Display<'a, 'd> {
    display: Ili9341Display<'a, 'd>,
}

impl<'a, 'd> Display<'a, 'd> {
    pub(crate) async fn new(
        raw_spi: &'a Mutex<NoopRawMutex, Spi<'d, Async>>,
        cs_pin: Output,
        dc_pin: Output,
        mut rst_pin: Output,
    ) -> Self {
        let spi = SpiDeviceWithConfig::new(
            raw_spi,
            cs_pin,
            Config::default().with_frequency(Rate::from_mhz(60)),
        );
        let di = AsyncSpiInterface::new(spi, dc_pin);
        let mut delay = Delay::new();
        let display = AsyncBuilder::new(di)
            .orientation(Orientation::LandscapeFlipped)
            .color_order(ColorOrder::Bgr)
            .init(&mut rst_pin, &mut delay)
            .await
            .expect("Display initialization failed");
        Self { display }
    }

    pub async fn debug_input(
        &mut self,
        channel: CoordinatesReceiver,
        address: MessageReceiver,
        touch: TouchReceiver,
    ) {
        debug_input(self, channel, address, touch).await
    }

    #[allow(dead_code)]
    pub async fn control_display(&mut self, _channel: MessageReceiver) {
        self.display.clear_screen(0x0f00).await.unwrap();

        const WIDTH: usize = 320;
        const HEIGHT: usize = 240;

        // Allocate the canvas
        let mut frame_buffer = vec![0u16; WIDTH * HEIGHT];

        // Wrap our raw array buffer so embedded-graphics can draw onto it using DrawTargetOnBuffer
        let mut canvas = DrawBuffer::new(&mut frame_buffer, WIDTH);

        let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::GREEN);

        Text::new(
            "Ariel OS + ESP32-C3 + ILI9341\nasync display driver",
            Point::new(20, 40),
            text_style,
        )
        .draw(&mut canvas)
        .unwrap();

        self.display
            .write_pixels(0, 0, WIDTH as u16, HEIGHT as u16, frame_buffer)
            .await
            .expect("Failed to flush frame buffer to display");

        Timer::after_secs(5).await;

        self.display.clear_screen(0x0000).await.unwrap();
    }
}
struct DrawBuffer<'a> {
    buffer: &'a mut [u16],
    width: usize,
}

impl<'a> DrawBuffer<'a> {
    fn new(buffer: &'a mut [u16], width: usize) -> Self {
        Self { buffer, width }
    }
}

impl<'a> DrawTarget for DrawBuffer<'a> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            // Ensure pixel coordinate is physically within boundaries before writing
            if point.x >= 0 && point.y >= 0 {
                let x = point.x as usize;
                let y = point.y as usize;
                if x < self.width && (y * self.width + x) < self.buffer.len() {
                    // Convert Rgb565 into native u16 raw bit representation
                    self.buffer[y * self.width + x] = color.into_storage();
                }
            }
        }
        Ok(())
    }
}

impl<'a> OriginDimensions for DrawBuffer<'a> {
    fn size(&self) -> Size {
        Size::new(320, 240)
    }
}

impl DisplayTarget for Display<'_, '_> {
    async fn clear(&mut self, color: Rgb565) -> Result<(), ()> {
        self.display
            .clear_screen(color.into_storage())
            .await
            .map_err(|_| ())
    }

    async fn draw(
        &mut self,
        origin: Point,
        size: Size,
        pixels: impl IntoIterator<Item = Rgb565>,
    ) -> Result<(), ()> {
        let pixel_data = pixels
            .into_iter()
            .map(|c| c.into_storage())
            .take(size.width as usize * size.height as usize);
        self.display
            .write_pixels(
                origin.x as u16,
                origin.y as u16,
                origin.x as u16 + size.width as u16 - 1,
                origin.y as u16 + size.height as u16 - 1,
                pixel_data,
            )
            .await
            .map_err(|_| ())
    }
}
