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
