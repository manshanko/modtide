const PNG_HEADER: &[u8] = &[137, 80, 78, 71, 13, 10, 26, 10];

#[allow(dead_code)]
pub struct Png<'a> {
    pub buffer: &'a [u8],
    pub file_name: Option<&'a str>,
    pub index: usize,
}

pub struct ExtractPng<'a> {
    buffer: &'a [u8],
    offset: usize,
    index: usize,
}

impl<'a> ExtractPng<'a> {
    pub fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            offset: 0,
            index: 0,
        }
    }
}

impl<'a> Iterator for ExtractPng<'a> {
    type Item = Png<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let buf = self.buffer;
        let mut offset = self.offset;
        for window in buf[offset..].windows(8) {
            if window != PNG_HEADER {
                offset += 1;
                continue;
            }

            let start = offset;
            let mut file_name: Option<&str> = None;

            offset += 8;
            while offset < buf.len() {
                let mut arr = [0; 4];
                arr.copy_from_slice(&buf[offset..offset + 4]);
                let size = u32::from_be_bytes(arr) as usize;
                offset += 4;
                arr.copy_from_slice(&buf[offset..offset + 4]);
                let type_ = u32::from_be_bytes(arr);
                offset += 4;

                match type_ {
                    // IEND
                    0x49454E44 => {
                        offset += 4;
                        break;
                    }

                    // tEXt
                    0x74455874 if size > 14 => {
                        if let Some(file_name_) = buf[offset..offset + size].strip_prefix(b"File Name\0") {
                            file_name = std::str::from_utf8(file_name_).ok();
                        }
                    }

                    _ => (),
                }

                offset += size + 4;
            }

            let index = self.index;
            self.index += 1;
            self.offset = offset;

            return Some(Png {
                buffer: &buf[start..offset],
                file_name,
                index,
            });
        }

        self.offset = self.buffer.len();
        None
    }
}
