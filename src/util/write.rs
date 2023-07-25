use std::{io, str};

#[derive(Debug, Clone, Default)]
pub struct WriteByteCounter(pub usize);

impl io::Write for WriteByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct WriteCodepointCounter {
    buffer: [u8; 4],
    offset: u8,
    codepoints: usize,
    bytes: usize,
}

impl io::Write for WriteCodepointCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes += buf.len();

        if self.codepoints == usize::MAX {
            return Ok(buf.len());
        }

        for &byte in buf {
            self.buffer[self.offset as usize] = byte;

            if str::from_utf8(&self.buffer[0..self.offset as usize]).is_ok() {
                self.offset = 0;
                self.codepoints += 1;
            } else {
                self.offset += 1;

                if self.offset >= 4 {
                    self.codepoints = usize::MAX;
                    return Ok(buf.len());
                }
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl WriteCodepointCounter {
    pub fn codepoints(&self) -> Option<usize> {
        (self.codepoints != usize::MAX).then_some(self.codepoints)
    }

	pub fn bytes(&self) -> usize {
		self.bytes
	}
}
