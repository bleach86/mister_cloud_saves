// Ported from RetroArch's rzip_stream.c
// Original Copyright (C) 2010–2020 The RetroArch team
// Licensed under the MIT License
// Port Copyright (C) 2026 bleach86
// Licensed under the GNU General Public License v3.0

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use futures::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, Cursor};
use std::{
    cmp::min,
    io::{Error, ErrorKind, Read, Result as IoResult, SeekFrom, Write},
};

// Values RetroArch's rzip_stream.c uses
static RZIP_VERSION: u8 = 1;
static RZIP_COMPRESSION_LEVEL: u8 = 6;
static RZIP_DEFAULT_CHUNK_SIZE: u32 = 131072; // 128kb
static RZIP_HEADER_SIZE: usize = 20;
static RZIP_CHUNK_HEADER_SIZE: usize = 4;

const RZIP_MAGIC: [u8; 8] = [b'#', b'R', b'Z', b'I', b'P', b'v', RZIP_VERSION, b'#']; // "#RZIPv1#"

pub struct RzipStream {
    size: u64,
    rfile: Cursor<Vec<u8>>,
    chunk_size: u32,
    pub is_compressed: bool,
}

impl RzipStream {
    pub async fn new(file: Vec<u8>) -> IoResult<Self> {
        let mut rzip_stream = RzipStream {
            size: 0,
            rfile: Cursor::new(file),
            chunk_size: RZIP_DEFAULT_CHUNK_SIZE,
            is_compressed: false,
        };

        rzip_stream.read_headers().await?;

        Ok(rzip_stream)
    }

    fn bytes(&self) -> Vec<u8> {
        self.rfile.get_ref().clone()
    }

    async fn read_headers(&mut self) -> IoResult<()> {
        let mut header_bytes = [0u8; RZIP_HEADER_SIZE];

        // Read header bytes
        let length = self.rfile.read(&mut header_bytes).await?;

        // Check magic numbers
        if length < RZIP_HEADER_SIZE || &header_bytes[0..8] != RZIP_MAGIC {
            // Treat as uncompressed
            self.rfile.seek(SeekFrom::Start(0)).await?;
            self.size = self.rfile.get_ref().len() as u64;
            self.is_compressed = false;
            return Ok(());
        }

        // Read chunk size (bytes 8-11, little-endian)
        let chunk_size = u32::from_le_bytes([
            header_bytes[8],
            header_bytes[9],
            header_bytes[10],
            header_bytes[11],
        ]);
        if chunk_size == 0 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Invalid chunk size in RZIP header",
            ));
        }
        self.chunk_size = chunk_size;

        // Read total uncompressed size (bytes 12-19, little-endian)
        let size = u64::from_le_bytes([
            header_bytes[12],
            header_bytes[13],
            header_bytes[14],
            header_bytes[15],
            header_bytes[16],
            header_bytes[17],
            header_bytes[18],
            header_bytes[19],
        ]);
        if size == 0 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Invalid total size in RZIP header",
            ));
        }
        self.size = size;
        self.is_compressed = true;

        Ok(())
    }

    pub async fn inflate_file(&mut self) -> IoResult<Vec<u8>> {
        if !self.is_compressed {
            return Ok(self.bytes());
        }

        let mut inflated_bytes: Vec<u8> = Vec::with_capacity(self.size as usize);
        self.rfile
            .seek(SeekFrom::Start(RZIP_HEADER_SIZE as u64))
            .await?;

        let mut total_inflated: u64 = 0;

        while total_inflated < self.size {
            // Read chunk header (4 bytes)
            let mut chunk_header_bytes = [0u8; RZIP_CHUNK_HEADER_SIZE];
            self.rfile.read_exact(&mut chunk_header_bytes).await?;

            // Read compressed chunk
            let compressed_chunk_size = u32::from_le_bytes(chunk_header_bytes) as usize;
            let mut compressed_chunk = vec![0u8; compressed_chunk_size];
            self.rfile.read_exact(&mut compressed_chunk).await?;

            // Decompress chunk using zlib
            let mut chunk_decoder = ZlibDecoder::new(&compressed_chunk[..]);
            let mut decompressed_chunk = Vec::new();
            chunk_decoder.read_to_end(&mut decompressed_chunk)?;

            // Append decompressed chunk to output
            inflated_bytes.extend_from_slice(&decompressed_chunk);
            total_inflated += decompressed_chunk.len() as u64;
        }

        if total_inflated != self.size {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Decompressed size does not match expected size",
            ));
        }

        self.is_compressed = false;

        // Overwrite the internal buffer with the new data
        self.rfile.seek(SeekFrom::Start(0)).await?;
        self.rfile.write_all(&inflated_bytes).await?;
        self.rfile.flush().await?;

        Ok(inflated_bytes)
    }

    pub async fn deflate_file(&mut self) -> IoResult<Vec<u8>> {
        if self.is_compressed {
            return Ok(self.bytes());
        }

        let mut file_bytes = Vec::with_capacity(self.size as usize);
        self.rfile.seek(SeekFrom::Start(0)).await?;
        self.rfile.read_to_end(&mut file_bytes).await?;

        let mut output = Vec::new();

        // Write header (20 bytes)
        output.extend_from_slice(&RZIP_MAGIC); // 8 bytes
        output.extend_from_slice(&RZIP_DEFAULT_CHUNK_SIZE.to_le_bytes()); // 4 bytes
        output.extend_from_slice(&self.size.to_le_bytes()); // 8 bytes

        let mut offset = 0;
        while offset < file_bytes.len() {
            let end = min(offset + RZIP_DEFAULT_CHUNK_SIZE as usize, file_bytes.len());
            let chunk = &file_bytes[offset..end];

            // Compress chunk using zlib
            let mut encoder =
                ZlibEncoder::new(Vec::new(), Compression::new(RZIP_COMPRESSION_LEVEL.into()));
            encoder.write_all(chunk)?;
            let compressed = encoder.finish()?;

            // Write 4-byte chunk size
            output.extend_from_slice(&(compressed.len() as u32).to_le_bytes());

            // Write chunk data
            output.extend_from_slice(&compressed);

            offset = end;
        }

        self.is_compressed = true;

        // Overwrite the internal buffer with the new data
        self.rfile.seek(SeekFrom::Start(0)).await?;
        self.rfile.write_all(&output).await?;
        self.rfile.flush().await?;

        Ok(output)
    }
}
