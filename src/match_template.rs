use crate::declaration_messages::Template;
use crate::unquote::read_quoted_segment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    Shell,
    Raw,
}

pub fn match_template(line: &[u8], template: &Template, reading: Reading) -> Vec<Vec<String>> {
    let mut readings = Vec::new();

    if let Some(rest) = line.strip_prefix(template.literals[0].as_bytes()) {
        let mut slots = Vec::new();

        read_slot(rest, template, reading, &mut slots, &mut readings);
    }

    readings
}

fn read_slot(
    text: &[u8],
    template: &Template,
    reading: Reading,
    slots: &mut Vec<Vec<u8>>,
    readings: &mut Vec<Vec<String>>,
) {
    let slot = slots.len();
    let literal = template.literals[slot + 1].as_bytes();
    let is_last = slot + 1 == template.arguments.len();
    let mut take = |value: &[u8], after: &[u8], slots: &mut Vec<Vec<u8>>| {
        slots.push(value.to_vec());

        if is_last {
            readings.extend(ordered_of(slots, template));
        } else {
            read_slot(after, template, reading, slots, readings);
        }

        slots.pop();
    };

    match reading {
        Reading::Shell => {
            let mut word = Vec::new();
            let mut index = 0;

            while let Some(length) = read_quoted_segment(&text[index..], &mut word) {
                index += length;

                let rest = &text[index..];

                if is_last && rest == literal {
                    take(&word, &[], slots);
                } else if let Some(after) = rest.strip_prefix(literal).filter(|_| !is_last) {
                    take(&word, after, slots);
                }
            }
        }
        Reading::Raw if is_last => {
            if let Some(value) = text.strip_suffix(literal).filter(|value| !value.is_empty()) {
                take(value, &[], slots);
            }
        }
        Reading::Raw => {
            for index in 1..text.len() {
                if let Some(after) = text[index..].strip_prefix(literal) {
                    take(&text[..index], after, slots);
                }
            }
        }
    }
}

fn ordered_of(slots: &[Vec<u8>], template: &Template) -> Option<Vec<String>> {
    let mut arguments = vec![String::new(); slots.len()];

    for (slot, value) in slots.iter().enumerate() {
        arguments[template.arguments[slot]] = String::from_utf8(value.clone()).ok()?;
    }

    Some(arguments)
}

#[cfg(test)]
#[path = "match_template.test.rs"]
mod tests;
