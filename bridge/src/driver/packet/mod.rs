#[neli::neli_enum(serialized_type = "u8")]
pub enum Command {
    Unspec = 0,
    Exit = 1,
    Init = 2,
    Deinit = 3,
    GetGpioValue = 4,
    SetGpioValue = 5,
    SetGpioConfig = 6,
    SetGpioDirection = 7,
}
impl neli::consts::genl::Cmd for Command {}

#[neli::neli_enum(serialized_type = "u16")]
pub enum Attribute {
    Unspec = 0,
    Status = 1,
    Message = 2,
    VersionMajor = 3,
    VersionMinor = 4,
    VersionPatch = 5,
    UniqueId = 6,
    ChipLabel = 7,
    GpioCount = 8,
    GpioNames = 9,
    GpioPin = 10,
    GpioValue = 11,
    GpioConfig = 12,
    GpioDirection = 13,
}
impl neli::consts::genl::NlAttrType for Attribute {}

#[derive(Debug)]
pub enum Packet {
    Exit(Exit),
    GetGpioValue(GetGpioValue),
    SetGpioValue(SetGpioValue),
    SetGpioConfig(SetGpioConfig),
    SetGpioDirection(SetGpioDirection),
    Discard,
}

#[derive(Debug)]
pub struct Exit {
    pub message: String,
}
#[derive(Debug)]
pub struct GetGpioValue {
    pub pin: u32,
}
#[derive(Debug)]
pub struct SetGpioValue {
    pub pin: u32,
    pub value: GpioValue,
}
#[derive(Debug)]
pub struct SetGpioConfig {
    pub pin: u32,
    pub config: GpioConfig,
}
#[derive(Debug)]
pub struct SetGpioDirection {
    pub pin: u32,
    pub direction: GpioDirection,
}

#[derive(Debug, Copy, Clone, num_enum::TryFromPrimitive)]
#[repr(u32)]
pub enum Status {
    Ok = 0,
    NotSupported = 1,
    BrokenPipe = 2,
    ProtocolError = 3,
    Unknown = u32::MAX,
}

#[derive(Debug, Copy, Clone, num_enum::TryFromPrimitive)]
#[repr(u32)]
pub enum GpioValue {
    Low = 0,
    High = 1,
}

#[derive(Debug, Copy, Clone, num_enum::TryFromPrimitive)]
#[repr(u32)]
pub enum GpioDirection {
    Output = 0,
    Input = 1,
    Disabled = 2,
}

// https://github.com/torvalds/linux/blob/master/include/linux/pinctrl/pinconf-generic.h#L119
#[derive(Debug, Copy, Clone, num_enum::TryFromPrimitive)]
#[repr(u32)]
pub enum GpioConfig {
    BiasDisable = 1,
    BiasPullDown = 3,
    BiasPullUp = 5,
    DriveOpenDrain = 6,
    DriveOpenSource = 7,
    DrivePushPull = 8,
}
