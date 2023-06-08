# CPC GPIO Expander Secondary Component
The CPC GPIO Expander Secondary Component is a component part of the [CPC GPIO Expander](../README.md) and acts as a backend for the [CPC GPIO Expander Bridge](../bridge/README.md).

## Table of Contents
- [Installation](#installation)
  - [Simplicity Studio and Target Component](#simplicity-studio-and-target-component)
- [Configuration](#configuration)
  - [Common Configuration](#common-configuration)
  - [GPIO Instance Configuration](#gpio-instance-configuration)

## Installation

### Simplicity Studio and Target Component
To enable the use of pins available on the target device, follow these steps to install and make a GPIO available using [Simplicity Studio](https://www.silabs.com/developers/simplicity-studio) and the CPC GPIO Expander component:

1. In the Software component view, search for the CPC GPIO Expander component.
2. Install the `CPC GPIO Expander GPIO Instance` component and give it a name. Note that in case the CPC secondary component is not installed, you will be prompted to install the necessary dependency components and configure them before proceeding with the installation of the CPC GPIO instance.
3. Repeat the previous step to install as many GPIO instances as needed.
4. A configuration will be created for each GPIO instance. These configurations can be edited either from the Software component view or via the configuration file located in the project's config folder.
Once installed and configured, the host can use the available pins on the target device

## Configuration
### Common Configuration
After instantiating a GPIO, the CPC GPIO Expander module will be installed and a common configuration file will be added. The configuration settings can be edited either through the Software component view or by accessing the configuration file located in the project's config folder.

The following common configuration are available:


| Configuration                   | Description                                                                                        |
| ------------------------------- | -------------------------------------------------------------------------------------------------- |
| SL_CPC_GPIO_EXPANDER_CHIP_LABEL | Label of the chip under which the secondary device's GPIO instances will be displayed on the Host. |

### GPIO Instance Configuration
Every GPIO instantiated has its own configuration. The configuration settings can be edited either through the Software component view or by accessing the configuration file located in the project's config folder.

The following configuration exist for every GPIO instance:


| Configuration                           | Description                                                                                                                                                                  |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SL_CPC_GPIO_EXPANDER_GPIO_instance_NAME | This parameter specifies the name of the GPIO pin instance that is reported to the Host. It is used to identify and differentiate between different GPIO pins in the system. |
| SL_CPC_GPIO_EXPANDER_GPIO_instance_PORT | This parameter is used to specify the port of the pin that will be exposed and utilized by the host.                                                                         |
| SL_CPC_GPIO_EXPANDER_GPIO_instance_PIN  | This parameter is used to specify the pin number that will be exposed and utilized by the host.                                                                              |