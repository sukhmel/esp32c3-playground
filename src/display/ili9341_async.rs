extern crate alloc;

use crate::display::{DisplayTarget, debug_input};
use crate::inter_task::{CoordinatesReceiver, IpDisplayReceiver, TouchReceiver};
use ariel_os_hal::gpio::Output;
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDeviceWithConfig;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_graphics::geometry::Point;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
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
