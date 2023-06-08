# CPC GPIO Driver
The CPC GPIO Driver is a component part of the [CPC GPIO Expander](../README.md) and acts as a backend for the Linux GPIO subsystem and as a frontend for the [CPC GPIO Bridge](../bridge/README.md).

## Table of Contents
- [Installation](#installation)
  - [Kernel headers](#kernel-headers)
    - [Raspbian](#raspbian)
    - [Ubuntu](#ubuntu)
    - [Other distributions](#other-distributions)
  - [Building](#building)
  - [Loading (Manual)](#loading-manual)
  - [Loading (Automatic)](#loading-automatic)
- [Debugging](#debugging)
  - [Enable logging](#enable-logging)

## Installation

### Kernel headers

This driver has been tested under `v5.10` - `v6.1` and may or may not be compatible with newer releases of the Linux Kernel.

#### Raspbian

On Raspbian, the package manager only fetches the _latest_ kernel headers. 

This means you must keep your OS up to date to match the running kernel version:

`sudo apt update`, `sudo apt dist-upgrade -y` and reboot.

And then fetch the latest kernel headers:

`sudo apt install raspberrypi-kernel-headers`

If you do not wish to update your OS, you can manually download [old kernel headers](https://archive.raspberrypi.org/debian/pool/main/r/raspberrypi-firmware/) and install them:

`sudo apt install 	raspberrypi-kernel-headers_xxxx_xxxx.deb`

The matching tag release/kernel version can be [determined in different ways](https://github.com/HinTak/RaspberryPi-Dev/blob/master/Raspbian-Kernel-Releases.md).

#### Ubuntu

On Ubuntu, you can fetch the current kernel headers matching your currently running kernel:

`sudo apt install linux-headers-$(uname -r)`

#### Other distributions

Other distributions should have similar ways of acquiring their kernel headers but might use different package managers.

### Building

Invoke [make](https://www.gnu.org/software/make/) to build the driver:

`make`

This generates a kernel module (`cpc-gpio.ko`) which can then be loaded [manually](#loading-manual) or [automatically](#loading-automatic).

### Loading (Manual)

This step manually loads the driver, meaning it will not be running after a reboot:

`sudo insmod cpc-gpio.ko`

You may then unload the driver:

`sudo rmmod cpc-gpio`

Or simply reboot.

### Loading (Automatic)

These steps ensures the driver loads on subsequent boots:

Copy driver to appropriate folder:

``cp `pwd`/cpc-gpio.ko /lib/modules/`uname -r`/kernel/drivers/gpio``

Generate dependencies:

`sudo depmod`

Add the driver to be loaded at boot time and reboot:

`echo "cpc-gpio" | sudo tee -a /etc/modules`

You may then unload the driver and remove it from subsequents boots:

`sudo rmmod cpc-gpio`

Then remove `cpc-gpio` from `/etc/modules` and reboot. 

Optionally, you may also remove `cpc-gpio.ko` from ``/lib/modules/`uname -r`/kernel/drivers/gpio`` but it is not required.

## Debugging

### Enable logging
In the `Kbuild` file, add the following `DEBUG` flag:

`EXTRA_CFLAGS := -DDEBUG`

Rebuild and reload the driver.

Run `dmesg`.

You should see debugging logs during GPIO transactions.