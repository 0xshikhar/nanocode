use std::{error::Error, io};

use brotli_decompressor::{BrotliDecompressStream, BrotliResult, BrotliState, StandardAlloc};
use futures_util::{Stream, StreamExt, stream};

const HTTP_READ_CHUNK_SIZE: usize = 4096;

pub(crate) fn stream<S, B, E>(source: S) -> impl Stream<Item = io::Result<Vec<u8>>>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
    E: Error + Send + Sync + 'static,
{
    let decoder = Box::new(BrotliState::new(
        StandardAlloc::default(),
        StandardAlloc::default(),
        StandardAlloc::default(),
    ));
    let state = (
        Box::pin(source),
        decoder,
        None::<(B, usize)>,
        BrotliResult::NeedsMoreInput,
        false,
        0,
    );
    stream::try_unfold(
        state,
        |(mut source, mut decoder, mut pending, mut status, mut eof, mut total)| async move {
            loop {
                if matches!(status, BrotliResult::ResultSuccess) {
                    return Ok(None);
                }
                if pending.is_none() && matches!(status, BrotliResult::NeedsMoreInput) && !eof {
                    match source.next().await {
                        Some(Ok(bytes)) => pending = Some((bytes, 0)),
                        Some(Err(error)) => return Err(io::Error::other(error)),
                        None => eof = true,
                    }
                }
                let input = pending
                    .as_ref()
                    .map_or(&[][..], |(bytes, offset)| &bytes.as_ref()[*offset..]);
                let mut available_input = input.len();
                let mut input_offset = 0;
                let mut output = vec![0; HTTP_READ_CHUNK_SIZE];
                let mut available_output = output.len();
                let mut output_offset = 0;
                status = BrotliDecompressStream(
                    &mut available_input,
                    &mut input_offset,
                    input,
                    &mut available_output,
                    &mut output_offset,
                    &mut output,
                    &mut total,
                    &mut decoder,
                );
                if matches!(status, BrotliResult::ResultFailure) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid Brotli response body",
                    ));
                }
                if let Some((bytes, offset)) = pending.as_mut() {
                    *offset += input_offset;
                    if *offset == bytes.as_ref().len() {
                        pending = None;
                    }
                }
                if output_offset != 0 {
                    output.truncate(output_offset);
                    return Ok(Some((
                        output,
                        (source, decoder, pending, status, eof, total),
                    )));
                }
                if eof && matches!(status, BrotliResult::NeedsMoreInput) {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "truncated Brotli response body",
                    ));
                }
            }
        },
    )
}
