# Adalight in Rust

This is a port of a C++ port of the Processing (Java) driver for the AdaLight project to a Rust Windows application. This version runs without any UI, so you might want to use the preview UI in the original Processing application first to figure out your settings and then copy those variables to `AdaLight.conf.js`.

- Adalight guide: https://learn.adafruit.com/adalight-diy-ambient-tv-lighting/pieces?w=all#download-and-install
- DirectX 11 C++ references: https://msdn.microsoft.com/en-us/library/windows/desktop/ff476082(v=vs.85).aspx

I implemented the C++ application a few years ago: [Port the Processing driver program to C++](https://github.com/adafruit/Adalight/pull/11). _[The PR was never acknowledged, so I suspect the original project has been abandoned.]_ Since then, I also added support for [Open Pixel Control](http://openpixelcontrol.org/) servers in my [opc-integrate](https://github.com/wravery/Adalight/tree/opc-integrate) branch, which is what I use at home to project the edges of my screen to LED strips on the wall while gaming.

I plan to replace the C++ version at home with this project and add UI to help configure `AdaLight.conf.js` using [Tauri](https://tauri.studio/) going forward.

## About Adalight

The Adalight project was released by [adafruit](https://learn.adafruit.com/adalight-diy-ambient-tv-lighting/pieces?w=all#download-and-install) as a standalone DIY project:

```txt
"Adalight" is a do-it-yourself facsimile of the Philips Ambilight concept
for desktop computers and home theater PCs.  This is the host PC-side code
written in Processing, intended for use with a USB-connected Arduino
microcontroller running the accompanying LED streaming code.  Requires one
or more strands of Digital RGB LED Pixels (Adafruit product ID #322,
specifically the newer WS2801-based type, strand of 25) and a 5 Volt power
supply (such as Adafruit #276).  You may need to adapt the code and the
hardware arrangement for your specific display configuration.
Screen capture adapted from code by Cedrik Kiefer (processing.org forum)
```

The code is on [GitHub](https://github.com/adafruit/Adalight) under the LGPL 3.0 or later license:
```txt
--------------------------------------------------------------------
  This file is part of Adalight.

  Adalight is free software: you can redistribute it and/or modify
  it under the terms of the GNU Lesser General Public License as
  published by the Free Software Foundation, either version 3 of
  the License, or (at your option) any later version.

  Adalight is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Lesser General Public License for more details.

  You should have received a copy of the GNU Lesser General Public
  License along with Adalight.  If not, see
  <http://www.gnu.org/licenses/>.
--------------------------------------------------------------------
```

## License

While this doesn't share any implementation with the original [Adalight](https://github.com/adafruit/Adalight) project, it inherits a lot of runtime and configuration logic from it. Several of the documentation comments in the configuration file sample are copied directly from the Processing driver implementation. So this project also inherits the LGPL 3.0 or later license from the original Adalight project.

## OPC (Open Pixel Controller)

My home setup includes a Raspberry Pi running a [Fadecandy OPC server](https://github.com/scanlime/fadecandy) and driving a set of LED strips on the wall/ceiling around my PC monitor.

## WIP: "BobLight" Alpha-Blending

[@milkey-mouse](https://github.com/milkey-mouse) also experimented with an alpha-blending multi-client extension to the OPC protocol initially called "BobLight." Both the C++ and the Rust version of this driver support adding an alpha channel and streaming that to a server that supports it using the OPC `System exclusive (command 255)` with a system ID of `0xB0B`.

 We never finished the [server](https://github.com/milkey-mouse/BamboozLED), and it morphed into a compositing reverse-proxy instead of a completely different streaming protocol. Unless you want to pick up where we left off, you should avoid setting the `alphaChannel` property to `true` for any servers, it won't work with a standard OPC server.