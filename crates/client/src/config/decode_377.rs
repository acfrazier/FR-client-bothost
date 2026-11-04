//! Bounded, borrowed reads for revision-377 cache records.
//!
//! Unlike the network Packet, this cursor borrows each dat/idx record. It does
//! not copy indexed records or allocate a packet for each definition.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeErrorKind377 {
    MissingMember,
    UnexpectedEof,
    UnknownOpcode(i32),
    TrailingBytes(usize),
    InvalidValue(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
pub struct DecodeError377 {
    pub table: &'static str,
    pub record: usize,
    pub offset: usize,
    pub kind: DecodeErrorKind377,
}

impl fmt::Display for DecodeError377 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} record {} byte {}: {:?}",
            self.table, self.record, self.offset, self.kind
        )
    }
}

impl std::error::Error for DecodeError377 {}

pub struct Reader377<'a> {
    pub data: &'a [u8],
    pub pos: usize,
    pub table: &'static str,
    pub record: usize,
}

impl<'a> Reader377<'a> {
    pub fn new(data: &'a [u8], table: &'static str, record: usize) -> Self {
        Self {
            data,
            pos: 0,
            table,
            record,
        }
    }

    pub fn error(&self, kind: DecodeErrorKind377) -> DecodeError377 {
        DecodeError377 {
            table: self.table,
            record: self.record,
            offset: self.pos,
            kind,
        }
    }

    pub fn unknown(&self, opcode: i32) -> DecodeError377 {
        self.error(DecodeErrorKind377::UnknownOpcode(opcode))
    }

    pub fn take(&mut self, count: usize) -> Result<&'a [u8], DecodeError377> {
        let end = self
            .pos
            .checked_add(count)
            .filter(|&end| end <= self.data.len())
            .ok_or_else(|| self.error(DecodeErrorKind377::UnexpectedEof))?;
        let bytes = &self.data[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    pub fn g1(&mut self) -> Result<i32, DecodeError377> {
        Ok(i32::from(self.take(1)?[0]))
    }
    pub fn g1b(&mut self) -> Result<i32, DecodeError377> {
        Ok(self.g1()? as i8 as i32)
    }
    pub fn g2(&mut self) -> Result<i32, DecodeError377> {
        let b = self.take(2)?;
        Ok(i32::from(u16::from_be_bytes([b[0], b[1]])))
    }
    pub fn g2b(&mut self) -> Result<i32, DecodeError377> {
        Ok(self.g2()? as i16 as i32)
    }
    pub fn g3(&mut self) -> Result<i32, DecodeError377> {
        let b = self.take(3)?;
        Ok((i32::from(b[0]) << 16) | (i32::from(b[1]) << 8) | i32::from(b[2]))
    }
    pub fn g4(&mut self) -> Result<i32, DecodeError377> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn gjstr(&mut self) -> Result<String, DecodeError377> {
        let len = self.data[self.pos..]
            .iter()
            .position(|&b| b == 10)
            .ok_or_else(|| self.error(DecodeErrorKind377::UnexpectedEof))?;
        let bytes = self.take(len + 1)?;
        // Packet.gjstr uses one Java character per unsigned cache byte.
        Ok(bytes[..len].iter().map(|&b| char::from(b)).collect())
    }
    pub fn gjstrraw(&mut self) -> Result<Vec<u8>, DecodeError377> {
        let len = self.data[self.pos..]
            .iter()
            .position(|&b| b == 10)
            .ok_or_else(|| self.error(DecodeErrorKind377::UnexpectedEof))?;
        Ok(self.take(len + 1)?[..len].to_vec())
    }
    pub fn finish(&self) -> Result<(), DecodeError377> {
        let remaining = self.data.len() - self.pos;
        if remaining == 0 {
            Ok(())
        } else {
            Err(self.error(DecodeErrorKind377::TrailingBytes(remaining)))
        }
    }
}
