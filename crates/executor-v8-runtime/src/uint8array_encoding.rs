//! One V8-owned ASCII allocation per encoding, with no IPC or host resources.

fn range_error(scope: &mut v8::HandleScope, message: &str, code: &str) {
    let message = v8::String::new(scope, message).unwrap();
    let error = v8::Exception::range_error(scope, message);
    let object = error.to_object(scope).unwrap();
    let key = v8::String::new(scope, "code").unwrap();
    let value = v8::String::new(scope, code).unwrap();
    object.set(scope, key.into(), value.into());
    scope.throw_exception(error);
}

pub(crate) fn encode<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: &v8::FunctionCallbackArguments<'s>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let input = v8::Local::<v8::Uint8Array>::try_from(args.get(0))
        .map_err(|_| "Uint8Array encoding requires a Uint8Array")?;
    let buffer = input
        .buffer(scope)
        .ok_or("Uint8Array encoding has no backing buffer")?;
    if buffer.was_detached() {
        return Err("Uint8Array encoding requires an attached buffer".into());
    }
    if !args.get(3).is_uint32() {
        return Err("Uint8Array encoding requires its captured length".into());
    }
    let length = args
        .get(3)
        .uint32_value(scope)
        .ok_or("Invalid Uint8Array encoding length")? as usize;
    if length > input.byte_length() {
        return Err("Uint8Array encoding input changed before copying".into());
    }
    let mode = args
        .get(1)
        .int32_value(scope)
        .ok_or("Uint8Array encoding requires a format")?;
    if !(0..=2).contains(&mode) {
        return Err("Unsupported Uint8Array encoding format".into());
    }
    let omit_padding = args.get(2).boolean_value(scope);
    let remainder = length % 3;
    let output_length = if mode == 0 {
        length.checked_mul(2)
    } else {
        length
            .checked_add(2)
            .and_then(|n| (n / 3).checked_mul(4))
            .and_then(|n| {
                n.checked_sub(if omit_padding && remainder != 0 {
                    3 - remainder
                } else {
                    0
                })
            })
    };
    let output_length = output_length
        .filter(|n| *n <= v8::String::MAX_LENGTH)
        .ok_or("Uint8Array encoded string exceeds V8's maximum string length")?;
    if output_length == 0 {
        return Ok(v8::String::empty(scope).into());
    }
    if let Err(error) = crate::isolate::admit_external_allocation(scope, output_length) {
        range_error(scope, &error, "ERR_AGENTOS_V8_EXTERNAL_MEMORY_LIMIT");
        return Err(error);
    }
    if scope.is_execution_terminating() {
        return Err("Uint8Array encoding interrupted".into());
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(output_length)
        .map_err(|error| format!("Uint8Array encoding allocation failed: {error}"))?;
    output.resize(output_length, 0);
    if input.copy_contents(&mut output[..length]) != length {
        return Err("Uint8Array encoding input changed during copy".into());
    }
    let isolate = scope.thread_safe_handle();
    // Copy into the final allocation, then expand backwards. No second input
    // or output-sized allocation is live, including for shared/offset views.
    if mode == 0 {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for index in (0..length).rev() {
            if index % 32768 == 0 && isolate.is_execution_terminating() {
                return Err("Uint8Array encoding interrupted".into());
            }
            let byte = output[index];
            output[2 * index] = HEX[(byte >> 4) as usize];
            output[2 * index + 1] = HEX[(byte & 15) as usize];
        }
    } else {
        let alphabet = if mode == 2 {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        } else {
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        };
        let groups = length / 3;
        if remainder != 0 {
            let source = groups * 3;
            let destination = groups * 4;
            let first = output[source];
            let second = if remainder == 2 {
                output[source + 1]
            } else {
                0
            };
            output[destination] = alphabet[(first >> 2) as usize];
            output[destination + 1] = alphabet[(((first & 3) << 4) | (second >> 4)) as usize];
            if remainder == 2 {
                output[destination + 2] = alphabet[((second & 15) << 2) as usize];
            } else if !omit_padding {
                output[destination + 2] = b'=';
            }
            if !omit_padding {
                output[destination + 3] = b'=';
            }
        }
        for group in (0..groups).rev() {
            if group % 16384 == 0 && isolate.is_execution_terminating() {
                return Err("Uint8Array encoding interrupted".into());
            }
            let source = group * 3;
            let first = output[source];
            let second = output[source + 1];
            let third = output[source + 2];
            let destination = group * 4;
            output[destination] = alphabet[(first >> 2) as usize];
            output[destination + 1] = alphabet[(((first & 3) << 4) | (second >> 4)) as usize];
            output[destination + 2] = alphabet[(((second & 15) << 2) | (third >> 6)) as usize];
            output[destination + 3] = alphabet[(third & 63) as usize];
        }
    }
    v8::String::new_external_onebyte(scope, output.into_boxed_slice())
        .map(Into::into)
        .ok_or_else(|| "Unable to attach Uint8Array encoded string to V8".into())
}
