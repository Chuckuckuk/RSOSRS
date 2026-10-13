//! Big-endian cursor over a byte slice, with the cache's "smart" encodings.
//! Reads past the end return zeros rather than panicking (cache blobs are trusted
//! but truncated reads should not take the viewer down).

pub struct Buf<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Buf<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Buf { data, pos: 0 }
    }
    pub fn at(data: &'a [u8], pos: usize) -> Self {
        Buf { data, pos }
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    pub fn u8(&mut self) -> u8 {
        let v = self.data.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        v
    }
    pub fn i8(&mut self) -> i8 {
        self.u8() as i8
    }
    pub fn u16(&mut self) -> u16 {
        (self.u8() as u16) << 8 | self.u8() as u16
    }
    pub fn i16(&mut self) -> i16 {
        self.u16() as i16
    }
    pub fn u24(&mut self) -> u32 {
        (self.u8() as u32) << 16 | (self.u8() as u32) << 8 | self.u8() as u32
    }
    pub fn u32(&mut self) -> u32 {
        (self.u16() as u32) << 16 | self.u16() as u32
    }
    pub fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    pub fn peek(&self) -> u8 {
        self.data.get(self.pos).copied().unwrap_or(0)
    }
    /// 1 byte if < 128 else 2 bytes (value - 32768).
    pub fn smart_u(&mut self) -> u32 {
        if self.peek() < 128 {
            self.u8() as u32
        } else {
            (self.u16() as u32).wrapping_sub(32768)
        }
    }
    /// Signed variant: 1 byte (-64..63) or 2 bytes (-16384..16383).
    pub fn smart_s(&mut self) -> i32 {
        if self.peek() < 128 {
            self.u8() as i32 - 64
        } else {
            self.u16() as i32 - 49152
        }
    }
    /// u16 normally, or a 31-bit int if the top bit of the first byte is set.
    pub fn big_smart(&mut self) -> i32 {
        if self.peek() as i8 >= 0 {
            self.u16() as i32
        } else {
            (self.u32() & 0x7fff_ffff) as i32
        }
    }
    /// Sum of 32767-chunks, used for loc id deltas.
    pub fn smart_chain(&mut self) -> u32 {
        let mut total = 0;
        let mut v = self.smart_u();
        while v == 32767 {
            total += 32767;
            v = self.smart_u();
        }
        total + v
    }
    /// Null-terminated cp1252 string.
    pub fn string(&mut self) -> String {
        let mut s = String::new();
        loop {
            let b = self.u8();
            if b == 0 || self.pos > self.data.len() {
                break;
            }
            s.push(b as char);
        }
        s
    }
}
