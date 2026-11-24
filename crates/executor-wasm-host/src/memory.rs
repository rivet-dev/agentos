//! Checked guest-memory codecs.

use crate::WasmGuest;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestMemoryError {
    MissingMemory,
    AddressOverflow,
    OutOfBounds,
    InvalidUtf8,
}

impl std::fmt::Display for GuestMemoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MissingMemory => "guest module does not export linear memory as `memory`",
            Self::AddressOverflow => "guest memory range overflows the host address space",
            Self::OutOfBounds => "guest memory range is out of bounds",
            Self::InvalidUtf8 => "guest string is not valid UTF-8",
        })
    }
}

impl std::error::Error for GuestMemoryError {}

pub fn validate_range<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
    length: usize,
) -> Result<Range<usize>, GuestMemoryError> {
    let start = usize::try_from(pointer).map_err(|_| GuestMemoryError::AddressOverflow)?;
    let end = start
        .checked_add(length)
        .ok_or(GuestMemoryError::AddressOverflow)?;
    if end > caller.memory_size()? {
        return Err(GuestMemoryError::OutOfBounds);
    }
    Ok(start..end)
}

pub fn read_bytes<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
    length: usize,
) -> Result<Vec<u8>, GuestMemoryError> {
    let range = validate_range(caller, pointer, length)?;
    caller.memory_read(range)
}

pub fn read_string<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
    length: usize,
) -> Result<String, GuestMemoryError> {
    String::from_utf8(read_bytes(caller, pointer, length)?)
        .map_err(|_| GuestMemoryError::InvalidUtf8)
}

pub fn write_bytes<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
    bytes: &[u8],
) -> Result<(), GuestMemoryError> {
    let range = validate_range(caller, pointer, bytes.len())?;
    caller.memory_write(range, bytes)
}

pub fn write_u32<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
    value: u32,
) -> Result<(), GuestMemoryError> {
    write_bytes(caller, pointer, &value.to_le_bytes())
}

pub fn write_u64<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
    value: u64,
) -> Result<(), GuestMemoryError> {
    write_bytes(caller, pointer, &value.to_le_bytes())
}

pub fn read_u32<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
) -> Result<u32, GuestMemoryError> {
    let bytes = read_bytes(caller, pointer, 4)?;
    Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
}

pub fn read_u64<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointer: u32,
) -> Result<u64, GuestMemoryError> {
    let bytes = read_bytes(caller, pointer, 8)?;
    Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
}

pub fn validate_string_table_outputs<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointers: u32,
    buffer: u32,
    strings: &[Vec<u8>],
) -> Result<(), GuestMemoryError> {
    let pointer_bytes = strings
        .len()
        .checked_mul(4)
        .ok_or(GuestMemoryError::AddressOverflow)?;
    let buffer_bytes = strings.iter().try_fold(0usize, |total, value| {
        total
            .checked_add(value.len())
            .ok_or(GuestMemoryError::AddressOverflow)
    })?;
    validate_range(caller, pointers, pointer_bytes)?;
    validate_range(caller, buffer, buffer_bytes)?;
    Ok(())
}

pub fn write_string_table<C: WasmGuest + ?Sized>(
    caller: &mut C,
    pointers: u32,
    buffer: u32,
    strings: &[Vec<u8>],
) -> Result<(), GuestMemoryError> {
    validate_string_table_outputs(caller, pointers, buffer, strings)?;
    let mut offset = 0usize;
    for (index, value) in strings.iter().enumerate() {
        let value_pointer = usize::try_from(buffer)
            .ok()
            .and_then(|base| base.checked_add(offset))
            .and_then(|pointer| u32::try_from(pointer).ok())
            .ok_or(GuestMemoryError::AddressOverflow)?;
        let pointer_slot = usize::try_from(pointers)
            .ok()
            .and_then(|base| base.checked_add(index.saturating_mul(4)))
            .and_then(|pointer| u32::try_from(pointer).ok())
            .ok_or(GuestMemoryError::AddressOverflow)?;
        write_u32(caller, pointer_slot, value_pointer)?;
        write_bytes(caller, value_pointer, value)?;
        offset = offset
            .checked_add(value.len())
            .ok_or(GuestMemoryError::AddressOverflow)?;
    }
    Ok(())
}
