Repository to fiddle around with Espressif's ESP32-C3-DevKitM-1 board and some of the hardware I had for a long time but
never quite came up to be able to use.

AGM1264K display turned out to be too hard for me. Besides, it looks like it is actually an AGM1264F display. 
The datasheet is also a bit sparse for me.

Another display is something like 2.4inch 320x240 SPI Serial TFT LCD Module Display With Driver IC ILI9341 in shops, not
sure if it has a more specific name. It turned out to work well and had an existing driver.

I still want to try communicating with sensors, but I can't find any from my previous attempts, so it must wait, it 
seems.

Example of a board acting as an echo server and displaying the data:

<img alt="example" height="800" src="example.webm"/>