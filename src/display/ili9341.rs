//! When the display clock is slow, drawing takes a lot of time and happens synchronously, so other
//! tasks are blocked. Maybe I need to switch to an async display implementation to check if that
//! one does really transfer data while not blocking other tasks.

use crate::display::{DisplayTarget, debug_input, print_text};
use crate::inter_task::{CoordinatesReceiver, MessageReceiver};
use ariel_os_hal::gpio::Output;
use core::cell::RefCell;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;
use embedded_hal_bus::spi::RefCellDevice;
use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::spi::master::Spi;
use mipidsi::interface::SpiInterface;
use mipidsi::options::{ColorOrder, Rotation};
use mipidsi::{Builder, Display as DisplayImpl, models::ILI9341Rgb565, options::Orientation};

type DisplayAlias<'a, 'd, 's> = DisplayImpl<
    SpiInterface<'a, RefCellDevice<'s, Spi<'d, Blocking>, Output, Delay>, Output>,
    ILI9341Rgb565,
    Output,
>;

pub struct Display<'a, 'd, 's> {
    display: DisplayAlias<'a, 'd, 's>,
}

impl<'a, 'd, 's> Display<'a, 'd, 's> {
    pub fn new(
        raw_spi: &'s RefCell<Spi<'d, Blocking>>,
        cs_pin: Output,
        dc_pin: Output,
        rst_pin: Output,
        buffer: &'a mut [u8; 512],
    ) -> Self {
        let spi = RefCellDevice::new(raw_spi, cs_pin, Delay::new()).unwrap();
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
        print_text(&mut self.display, channel).await
    }

    pub async fn debug_input(
        &mut self,
        channel: CoordinatesReceiver,
        address: MessageReceiver,
        touch: TouchReceiver,
    ) {
        debug_input(self, channel, address, touch).await
    }
}

impl DisplayTarget for Display<'_, '_, '_> {
    async fn clear(&mut self, color: Rgb565) -> Result<(), ()> {
        self.display.clear(color).map_err(|_| ())
    }

    async fn draw(
        &mut self,
        origin: Point,
        size: Size,
        pixels: impl IntoIterator<Item = Rgb565>,
    ) -> Result<(), ()> {
        self.display
            .fill_contiguous(&Rectangle::new(origin, size), pixels)
            .map_err(|_| ())
    }
}
