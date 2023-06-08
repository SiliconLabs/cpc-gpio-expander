use anyhow::{bail, Result};
use thiserror::Error;

use crate::utils;

#[cfg(test)]
mod tests;

#[derive(
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    num_enum::TryFromPrimitive,
    PartialEq,
    Copy,
    Clone,
    Debug,
)]
#[repr(u8)]
enum HostCmd {
    GetVersion = 0,
    GetUniqueId = 1,
    GetChipLabel = 2,
    GetGpioCount = 3,
    GetGpioName = 4,
    GetGpioValue = 5,
    SetGpioValue = 6,
    SetGpioConfig = 7,
    SetGpioDirection = 8,
    UnknownCmd = SecondaryCmd::VersionIs as u8 - 1,
}

#[derive(
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    num_enum::TryFromPrimitive,
    Copy,
    Clone,
    Debug,
)]
#[repr(u8)]
pub enum SecondaryCmd {
    VersionIs = 128,
    StatusIs = 129,
    UniqueIdIs = 130,
    ChipLabelIs = 131,
    GpioCountIs = 132,
    GpioNameIs = 133,
    GpioValueIs = 134,
    UnsupportedCmdIs = u8::MAX,
}

#[derive(serde::Serialize, Copy)]
#[repr(C, packed)]
pub struct Header<T: Copy + Clone + std::fmt::Debug> {
    pub cmd: T,
    len: u8,
}
impl<T: Copy + Clone + std::fmt::Debug> Header<T> {
    fn new(cmd: T, len: u8) -> Self {
        Self { cmd, len }
    }
    fn len(packet_len: usize) -> u8 {
        (packet_len - std::mem::size_of::<Header<T>>()) as u8
    }
}
impl<T: Copy + std::fmt::Debug> Clone for Header<T> {
    fn clone(&self) -> Self {
        Self {
            cmd: self.cmd,
            len: self.len,
        }
    }
}
impl<T: Copy + std::fmt::Debug> std::fmt::Debug for Header<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cmd = self.cmd;
        f.debug_struct("Header")
            .field("cmd", &cmd)
            .field("len", &self.len)
            .finish()
    }
}

#[derive(serde::Serialize, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct HostHeader {
    seq: u8,
}
impl HostHeader {
    fn new(seq: &mut u8) -> Self {
        *seq = seq.wrapping_add(1);
        Self { seq: *seq }
    }
}

#[derive(
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    num_enum::TryFromPrimitive,
    PartialEq,
    Error,
    Copy,
    Clone,
    Debug,
)]
#[repr(u8)]
pub enum Status {
    #[error("Ok")]
    Ok = 0,
    #[error("NotSupported")]
    NotSupported = 1,
    #[error("InvalidPin")]
    InvalidPin = 2,
    #[error("Unknown")]
    Unknown = u8::MAX,
}

#[derive(serde::Serialize, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct SecondaryHeader {
    pub seq: u8,
}
impl SecondaryHeader {
    fn new(seq: u8) -> Self {
        Self { seq }
    }
}

pub trait Serializer: serde::Serialize {
    fn serialize(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(&self)?)
    }
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct GetVersion {
    header: Header<HostCmd>,
}
impl Serializer for GetVersion {}
impl GetVersion {
    pub fn new() -> Self {
        Self {
            header: Header::new(HostCmd::GetVersion, 0),
        }
    }
}
#[repr(C, packed)]
pub struct VersionIs {
    header: Header<SecondaryCmd>,
    pub version: utils::Version,
}
impl VersionIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, header) = deserialize_header(input)?;
            let (remaining, major) = nom::number::complete::u8(remaining)?;
            let (remaining, minor) = nom::number::complete::u8(remaining)?;
            let (remaining, patch) = nom::number::complete::u8(remaining)?;
            let version = utils::Version {
                major,
                minor,
                patch,
            };
            Ok((remaining, Self { header, version }))
        }();

        match result {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

#[derive(Debug)]
#[repr(C, packed)]
pub struct UnsupportedCmdIs {
    header: Header<SecondaryCmd>,
    unsupported_cmd: HostCmd,
}
impl UnsupportedCmdIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, header) = deserialize_header(input)?;
            let (remaining, cmd) = nom::number::complete::u8(remaining)?;
            let unsupported_cmd = HostCmd::try_from(cmd).unwrap_or(HostCmd::UnknownCmd);
            Ok((
                remaining,
                Self {
                    header,
                    unsupported_cmd,
                },
            ))
        }();

        match result {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct GetGpioCount {
    header: Header<HostCmd>,
    host_header: HostHeader,
}
impl Serializer for GetGpioCount {}
impl GetGpioCount {
    pub fn new(seq: &mut u8) -> Self {
        let len = Header::<HostCmd>::len(std::mem::size_of::<Self>());
        Self {
            header: Header::new(HostCmd::GetGpioCount, len),
            host_header: HostHeader::new(seq),
        }
    }
}
#[derive(serde::Serialize)]
#[repr(C, packed)]
pub struct GpioCountIs {
    header: Header<SecondaryCmd>,
    secondary_header: SecondaryHeader,
    pub count: u8,
}
impl GpioCountIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, (header, secondary_header)) = deserialize_headers(input)?;
            let (remaining, count) = nom::number::complete::u8(remaining)?;
            Ok((
                remaining,
                Self {
                    header,
                    secondary_header,
                    count,
                },
            ))
        }();

        match result {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct GetGpioName {
    header: Header<HostCmd>,
    host_header: HostHeader,
    pin: u8,
}
impl Serializer for GetGpioName {}
impl GetGpioName {
    pub fn new(seq: &mut u8, pin: u8) -> Self {
        let len = Header::<HostCmd>::len(std::mem::size_of::<Self>());
        Self {
            header: Header::new(HostCmd::GetGpioName, len),
            host_header: HostHeader::new(seq),
            pin,
        }
    }
}
#[repr(C, packed)]
pub struct GpioNameIs {
    header: Header<SecondaryCmd>,
    secondary_header: SecondaryHeader,
    pub name: Result<String>,
}
impl GpioNameIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, (header, secondary_header)) = deserialize_headers(input)?;
            let name = || -> Result<String> {
                Ok(std::ffi::CStr::from_bytes_with_nul(remaining)?
                    .to_str()?
                    .to_string())
            }();
            Ok((
                remaining,
                Self {
                    header,
                    secondary_header,
                    name,
                },
            ))
        }();

        match result {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

#[derive(
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    num_enum::TryFromPrimitive,
    PartialEq,
    Copy,
    Clone,
    Debug,
)]
#[repr(u8)]
pub enum GpioValue {
    Low = 0,
    High = 1,
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct GetGpioValue {
    header: Header<HostCmd>,
    host_header: HostHeader,
    pin: u8,
}
impl Serializer for GetGpioValue {}
impl GetGpioValue {
    pub fn new(seq: &mut u8, pin: u8) -> Self {
        let len = Header::<HostCmd>::len(std::mem::size_of::<Self>());
        Self {
            header: Header::new(HostCmd::GetGpioValue, len),
            host_header: HostHeader::new(seq),
            pin,
        }
    }
}
#[repr(C, packed)]
pub struct GpioValueIs {
    header: Header<SecondaryCmd>,
    pub secondary_header: SecondaryHeader,
    pub value: Result<GpioValue>,
}
impl GpioValueIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, (header, secondary_header)) = deserialize_headers(input)?;
            let (remaining, value) = nom::number::complete::u8(remaining)?;
            let value = || -> Result<GpioValue> { Ok(GpioValue::try_from(value)?) }();
            Ok((
                remaining,
                Self {
                    header,
                    secondary_header,
                    value,
                },
            ))
        }();

        match result {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct SetGpioValue {
    header: Header<HostCmd>,
    host_header: HostHeader,
    pin: u8,
    value: GpioValue,
}
impl Serializer for SetGpioValue {}
impl SetGpioValue {
    pub fn new(seq: &mut u8, pin: u8, value: GpioValue) -> Self {
        let len = Header::<HostCmd>::len(std::mem::size_of::<Self>());
        Self {
            header: Header::new(HostCmd::SetGpioValue, len),
            host_header: HostHeader::new(seq),
            pin,
            value,
        }
    }
}
#[repr(C, packed)]
pub struct StatusIs {
    header: Header<SecondaryCmd>,
    pub secondary_header: SecondaryHeader,
    pub status: Status,
}
impl StatusIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, (header, secondary_header)) = deserialize_headers(input)?;
            let (remaining, status) = nom::number::complete::u8(remaining)?;
            let status = Status::try_from(status).unwrap_or(Status::Unknown);
            Ok((
                remaining,
                Self {
                    header,
                    secondary_header,
                    status,
                },
            ))
        }();

        match result {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

#[derive(
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    num_enum::TryFromPrimitive,
    Copy,
    Clone,
    Debug,
)]
#[repr(u8)]
pub enum GpioConfig {
    BiasDisable = 0,
    BiasPullDown = 1,
    BiasPullUp = 2,
    DriveOpenDrain = 3,
    DriveOpenSource = 4,
    DrivePushPull = 5,
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct SetGpioConfig {
    header: Header<HostCmd>,
    host_header: HostHeader,
    pin: u8,
    config: GpioConfig,
}
impl Serializer for SetGpioConfig {}
impl SetGpioConfig {
    pub fn new(seq: &mut u8, pin: u8, config: GpioConfig) -> Self {
        let len = Header::<HostCmd>::len(std::mem::size_of::<Self>());
        Self {
            header: Header::new(HostCmd::SetGpioConfig, len),
            host_header: HostHeader::new(seq),
            pin,
            config,
        }
    }
}

#[derive(
    serde_repr::Serialize_repr,
    serde_repr::Deserialize_repr,
    num_enum::TryFromPrimitive,
    Copy,
    Clone,
    Debug,
)]
#[repr(u8)]
pub enum GpioDirection {
    Output = 0,
    Input = 1,
    Disabled = 2,
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct SetGpioDirection {
    header: Header<HostCmd>,
    host_header: HostHeader,
    pin: u8,
    direction: GpioDirection,
}
impl Serializer for SetGpioDirection {}
impl SetGpioDirection {
    pub fn new(seq: &mut u8, pin: u8, direction: GpioDirection) -> Self {
        let len = Header::<HostCmd>::len(std::mem::size_of::<Self>());
        Self {
            header: Header::new(HostCmd::SetGpioDirection, len),
            host_header: HostHeader::new(seq),
            pin,
            direction,
        }
    }
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct GetUniqueId {
    header: Header<HostCmd>,
    host_header: HostHeader,
}
impl Serializer for GetUniqueId {}
impl GetUniqueId {
    pub fn new(seq: &mut u8) -> Self {
        let len = Header::<HostCmd>::len(std::mem::size_of::<Self>());
        Self {
            header: Header::new(HostCmd::GetUniqueId, len),
            host_header: HostHeader::new(seq),
        }
    }
}
#[repr(C, packed)]
pub struct UniqueIdIs {
    header: Header<SecondaryCmd>,
    secondary_header: SecondaryHeader,
    pub unique_id: u64,
}
impl UniqueIdIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, (header, secondary_header)) = deserialize_headers(input)?;
            let (remaining, unique_id) = nom::number::complete::le_u64(remaining)?;
            Ok((
                remaining,
                Self {
                    header,
                    secondary_header,
                    unique_id,
                },
            ))
        };

        match result() {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

#[derive(serde::Serialize, Debug)]
#[repr(C, packed)]
pub struct GetChipLabel {
    header: Header<HostCmd>,
    host_header: HostHeader,
}
impl Serializer for GetChipLabel {}
impl GetChipLabel {
    pub fn new(seq: &mut u8) -> Self {
        let len = (std::mem::size_of::<Self>() - std::mem::size_of::<Header<HostCmd>>()) as u8;
        Self {
            header: Header::new(HostCmd::GetChipLabel, len),
            host_header: HostHeader::new(seq),
        }
    }
}
#[repr(C, packed)]
pub struct ChipLabelIs {
    header: Header<SecondaryCmd>,
    secondary_header: SecondaryHeader,
    pub chip_label: Result<String>,
}
impl ChipLabelIs {
    pub fn deserialize(input: &[u8]) -> Result<Self> {
        let result = || -> nom::IResult<&[u8], Self> {
            let (remaining, (header, secondary_header)) = deserialize_headers(input)?;
            let chip_label = || -> Result<String> {
                Ok(std::ffi::CStr::from_bytes_with_nul(remaining)?
                    .to_str()?
                    .to_string())
            }();
            Ok((
                remaining,
                Self {
                    header,
                    secondary_header,
                    chip_label,
                },
            ))
        };

        match result() {
            Ok(tuple) => Ok(tuple.1),
            Err(err) => bail!("{}", err),
        }
    }
}

pub fn split(input: &[u8]) -> Result<Vec<Vec<u8>>> {
    let result = || -> nom::IResult<&[u8], Vec<Vec<u8>>> {
        let mut packets = vec![];
        let mut packet;
        let mut remaining = input;
        let mut cmd;
        let mut len;
        let mut payload;

        while !remaining.is_empty() {
            (remaining, cmd) = nom::number::complete::u8(remaining)?;
            (remaining, len) = nom::number::complete::u8(remaining)?;
            (remaining, payload) = nom::bytes::complete::take(len)(remaining)?;
            packet = [vec![cmd, len], payload.to_vec()].concat();
            packets.append(&mut vec![packet]);
        }

        Ok((remaining, packets))
    }();

    match result {
        Ok(tuple) => Ok(tuple.1),
        Err(err) => bail!("{}", err),
    }
}

pub fn try_deserialize_cmd(input: &[u8]) -> Result<SecondaryCmd> {
    let result =
        || -> nom::IResult<&[u8], Result<SecondaryCmd, num_enum::TryFromPrimitiveError<SecondaryCmd>>> {
            let (remaining, cmd) = nom::number::complete::u8(input)?;
            Ok((remaining, SecondaryCmd::try_from(cmd)))
        }();

    match result {
        Ok(tuple) => Ok(tuple.1?),
        Err(err) => bail!("{}", err),
    }
}

pub fn deserialize_headers(
    input: &[u8],
) -> nom::IResult<&[u8], (Header<SecondaryCmd>, SecondaryHeader)> {
    let (remaining, header) = deserialize_header(input)?;
    let (remaining, secondary_header) = deserialize_secondary_header(remaining)?;
    Ok((remaining, (header, secondary_header)))
}

fn deserialize_cmd(input: &[u8]) -> nom::IResult<&[u8], SecondaryCmd> {
    let (remaining, cmd) = nom::number::complete::u8(input)?;
    let cmd = SecondaryCmd::try_from(cmd).unwrap_or(SecondaryCmd::UnsupportedCmdIs);
    Ok((remaining, cmd))
}

fn deserialize_header(input: &[u8]) -> nom::IResult<&[u8], Header<SecondaryCmd>> {
    let (remaining, cmd) = deserialize_cmd(input)?;
    let (remaining, len) = nom::number::complete::u8(remaining)?;
    Ok((remaining, Header::new(cmd, len)))
}

fn deserialize_secondary_header(input: &[u8]) -> nom::IResult<&[u8], SecondaryHeader> {
    let (remaining, seq) = nom::number::complete::u8(input)?;
    Ok((remaining, SecondaryHeader::new(seq)))
}
