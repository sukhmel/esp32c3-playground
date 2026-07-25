extern crate alloc;

use crate::display::{DisplayTarget, calibrate_touchscreen, debug_input};
use crate::inter_task::{CoordinatesReceiver, IpDisplayReceiver, TouchReceiver};
use ariel_os_hal::gpio::Output;
use core::iter;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDeviceWithConfig;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;
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
        address: IpDisplayReceiver,
        touch: TouchReceiver,
    ) {
        debug_input(self, channel, address, touch).await
    }

    pub async fn calibrate_touchscreen(&mut self, touch: TouchReceiver) {
        calibrate_touchscreen(self, touch).await
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

impl DrawTarget for Display<'_, '_> {
    type Color = Rgb565;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for pixel in pixels {
            embassy_futures::block_on(self.display.write_pixels(
                pixel.0.x as u16,
                pixel.0.y as u16,
                pixel.0.x as u16,
                pixel.0.y as u16,
                iter::once(pixel.1.into_storage()),
            ))
            .map_err(|_| ())?
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        embassy_futures::block_on(self.draw(area.top_left, area.size, colors))
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        embassy_futures::block_on(self.display.fill_rect(
            area.top_left.x as u16,
            area.top_left.y as u16,
            (area.top_left.x + area.size.width as i32) as u16,
            (area.top_left.y + area.size.height as i32) as u16,
            color.into_storage(),
        ))
        .map_err(|_| ())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        embassy_futures::block_on(<Self as DisplayTarget>::clear(self, color))
    }
}

impl Dimensions for Display<'_, '_> {
    fn bounding_box(&self) -> Rectangle {
        let dimensions = self.display.options().display_dimensions();
        Rectangle::new(
            Point::new(0, 0),
            Size::new(dimensions.0 as u32, dimensions.1 as u32),
        )
    }
}
