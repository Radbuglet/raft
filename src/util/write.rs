use std::io;

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
    codepoints: usize,
    buffer: [u8; 4],
    buffer_offset: u8,
}

impl io::Write for WriteCodepointCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for &byte in buf {
            self.buffer[self.buffer_offset as usize] = byte;

            if std::str::from_utf8(&self.buffer[0..self.buffer_offset as usize]).is_ok() {
                self.buffer_offset = 0;
                self.codepoints += 1;
            } else {
                self.buffer_offset += 1;
                assert!(self.buffer_offset < 4);
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl WriteCodepointCounter {
    pub fn codepoints(&self) -> usize {
        self.codepoints
    }
}
