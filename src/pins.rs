use ariel_os_hal::hal::peripherals;
ariel_os::hal::group_peripherals!(Peripherals {
    binary: BinaryPeripherals,
    analog: AnalogPeripherals,
});
ariel_os::hal::define_peripherals!(BinaryPeripherals {
    uart: UART0,
    ledc: LEDC,
    rmt: RMT,
    spi: SPI2,
    pin6: GPIO6,
    pin7: GPIO7,
    /// Needs to be disconnected from ground on boot
    pin8: GPIO8,
    /// Needs to be disconnected from ground on boot
    pin9: GPIO9,
    pin10: GPIO10,
    pin18: GPIO18,
    pin19: GPIO19,
    pin20: GPIO20,
    /// Will break debug log if connected to output
    pin21: GPIO21,
});
ariel_os::hal::define_peripherals!(AnalogPeripherals {
    adc1: ADC1,
    adc2: ADC2,
    pin0: GPIO0,
    pin1: GPIO1,
    pin2: GPIO2,
    pin3: GPIO3,
    pin4: GPIO4,
    pin5: GPIO5,
});
