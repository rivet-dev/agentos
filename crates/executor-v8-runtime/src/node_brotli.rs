//! Isolate-owned Brotli operations. No process, filesystem or network access.
use agentos_driver_tokio::accounting::{Reservation, ResourceClass, ResourceLedger};
use std::io::{self, Read, Write};
use std::sync::Arc;

struct Input<'a> {
    bytes: &'a [u8],
    isolate: v8::IsolateHandle,
}
impl Read for Input<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.isolate.is_execution_terminating() {
            return Err(io::Error::other("Brotli execution interrupted"));
        }
        self.bytes.read(bytes)
    }
}

struct Output {
    bytes: Vec<u8>,
    limit: usize,
    isolate: v8::IsolateHandle,
    resources: Arc<ResourceLedger>,
    reservation: Option<Reservation>,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.isolate.is_execution_terminating() {
            return Err(io::Error::other("Brotli execution interrupted"));
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(
                "ERR_BUFFER_TOO_LARGE: Brotli output exceeds maxOutputLength or limits.reactor.maxBridgeResponseBytes",
            ));
        }
        let extra = self
            .resources
            .reserve(ResourceClass::BridgeResponseBytes, bytes.len())
            .map_err(io::Error::other)?;
        if let Some(reservation) = &mut self.reservation {
            reservation
                .merge(extra)
                .map_err(|_| io::Error::other("Brotli response accounting failed"))?;
        } else {
            self.reservation = Some(extra);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn transform<'s>(
    scope: &mut v8::HandleScope<'s>,
    args: &v8::FunctionCallbackArguments<'s>,
    decode: bool,
    limit: usize,
    resources: Arc<ResourceLedger>,
) -> Result<v8::Local<'s, v8::Value>, String> {
    let view = v8::Local::<v8::ArrayBufferView>::try_from(args.get(0))
        .map_err(|_| "ERR_INVALID_ARG_TYPE: Brotli input must be a byte view")?;
    if view.byte_length() > limit {
        return Err("ERR_AGENTOS_BRIDGE_RESPONSE_LIMIT: Brotli input exceeds limits.reactor.maxBridgeResponseBytes".into());
    }
    let _input_reservation = resources
        .reserve(ResourceClass::BridgeRequestBytes, view.byte_length())
        .map_err(|error| error.to_string())?;
    let mut bytes = vec![0; view.byte_length()];
    view.copy_contents(&mut bytes);
    let requested = args.get(4).integer_value(scope).unwrap_or(limit as i64);
    let mut output = Output {
        bytes: Vec::new(),
        limit: limit.min(requested.max(1) as usize),
        isolate: scope.thread_safe_handle(),
        reservation: None,
        resources,
    };
    let mut input = Input {
        bytes: bytes.as_slice(),
        isolate: scope.thread_safe_handle(),
    };
    if decode {
        brotli::BrotliDecompress(&mut input, &mut output).map_err(|error| error.to_string())?;
    } else {
        let params = brotli::enc::BrotliEncoderParams {
            quality: args.get(1).int32_value(scope).unwrap_or(11),
            lgwin: args.get(2).int32_value(scope).unwrap_or(22),
            mode: match args.get(3).int32_value(scope).unwrap_or(0) {
                1 => brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_TEXT,
                2 => brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_FONT,
                _ => brotli::enc::backward_references::BrotliEncoderMode::BROTLI_MODE_GENERIC,
            },
            ..Default::default()
        };
        if !(0..=11).contains(&params.quality) || !(10..=24).contains(&params.lgwin) {
            return Err("ERR_OUT_OF_RANGE: invalid Brotli quality or window".into());
        }
        brotli::BrotliCompress(&mut input, &mut output, &params)
            .map_err(|error| error.to_string())?;
    }
    if output.bytes.len() >= output.limit.saturating_mul(4) / 5 {
        tracing::warn!(
            bytes = output.bytes.len(),
            limit = output.limit,
            "Brotli output approaches maxOutputLength or limits.reactor.maxBridgeResponseBytes"
        );
    }
    let length = output.bytes.len();
    let backing = v8::ArrayBuffer::new_backing_store_from_vec(output.bytes).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing);
    v8::Uint8Array::new(scope, buffer, 0, length)
        .map(Into::into)
        .ok_or_else(|| "Unable to allocate Brotli output".into())
}
