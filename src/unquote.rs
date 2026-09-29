pub fn unquote_shell(text: &[u8]) -> Option<(String, &[u8])> {
    let mut word: Vec<u8> = Vec::new();
    let mut index = 0;

    while index < text.len() {
        match text[index] {
            b'\'' => {
                let length = text[index + 1..].iter().position(|byte| *byte == b'\'')?;

                word.extend_from_slice(&text[index + 1..index + 1 + length]);

                index += length + 2;
            }
            b'$' if text.get(index + 1) == Some(&b'\'') => {
                index = decode_escaped(text, index + 2, b'\'', &mut word)?;
            }
            b'"' => {
                index = decode_double_quoted(text, index + 1, &mut word)?;
            }
            b'\\' if text.get(index + 1) == Some(&b'\'') => {
                word.push(b'\'');

                index += 2;
            }
            byte if byte.is_ascii_whitespace() => break,
            byte => {
                word.push(byte);

                index += 1;
            }
        }
    }

    if index == 0 {
        return None;
    }

    Some((String::from_utf8(word).ok()?, &text[index..]))
}

pub fn unquote_git(text: &str) -> Option<String> {
    if !text.starts_with('"') {
        return Some(text.to_string());
    }

    let bytes = text.as_bytes();
    let mut path: Vec<u8> = Vec::new();

    if decode_escaped(bytes, 1, b'"', &mut path)? != bytes.len() {
        return None;
    }

    String::from_utf8(path).ok()
}

fn decode_double_quoted(bytes: &[u8], start: usize, output: &mut Vec<u8>) -> Option<usize> {
    let mut index = start;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => return Some(index + 1),
            b'\\' if matches!(bytes.get(index + 1), Some(b'"' | b'\\' | b'$' | b'`')) => {
                output.push(bytes[index + 1]);

                index += 2;
            }
            byte => {
                output.push(byte);

                index += 1;
            }
        }
    }

    None
}

fn decode_escaped(
    bytes: &[u8],
    start: usize,
    terminator: u8,
    output: &mut Vec<u8>,
) -> Option<usize> {
    let mut index = start;

    while index < bytes.len() {
        let byte = bytes[index];

        if byte == terminator {
            return Some(index + 1);
        }

        if byte != b'\\' {
            output.push(byte);

            index += 1;

            continue;
        }

        let escaped = *bytes.get(index + 1)?;

        index += 2;

        match escaped {
            b'n' => output.push(b'\n'),
            b't' => output.push(b'\t'),
            b'r' => output.push(b'\r'),
            b'a' => output.push(0x07),
            b'b' => output.push(0x08),
            b'f' => output.push(0x0c),
            b'v' => output.push(0x0b),
            b'e' => output.push(0x1b),
            b'\\' | b'\'' | b'"' | b'?' => output.push(escaped),
            b'0'..=b'7' => {
                let mut value = u32::from(escaped - b'0');
                let mut digits = 1;

                while digits < 3 {
                    match bytes.get(index) {
                        Some(digit @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(digit - b'0');
                            index += 1;
                            digits += 1;
                        }
                        _ => break,
                    }
                }

                output.push(u8::try_from(value).ok()?);
            }
            _ => return None,
        }
    }

    None
}

#[cfg(test)]
#[path = "unquote.test.rs"]
mod tests;
