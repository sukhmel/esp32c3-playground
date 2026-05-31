use ariel_os_hal::hal::peripherals;
ariel_os::hal::define_peripherals!(Peripherals {
    ledc: LEDC,
    rmt: RMT,
    spi: SPI2,
    pin0: GPIO0,
    pin1: GPIO1,
    pin2: GPIO2,
    pin3: GPIO3,
    pin4: GPIO4,
    pin5: GPIO5,
    pin6: GPIO6,
    pin7: GPIO7,
    pin8: GPIO8,
    pin9: GPIO9, // needs to be disconnected from ground on boot
    pin10: GPIO10,
    pin18: GPIO18,
    pin19: GPIO19,
    pin20: GPIO20,
    pin21: GPIO21,
});
