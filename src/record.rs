use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) label: String,
    pub(crate) path: String,
    pub(crate) created: String,
    pub(crate) note: String,
}

pub(crate) fn tsv_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\t' | '\r' | '\n' => ' ',
            _ => ch,
        })
        .collect()
}

pub(crate) fn parse_record(line: &str) -> Record {
    let mut fields = line.splitn(4, '\t');
    Record {
        label: fields.next().unwrap_or_default().to_string(),
        path: fields.next().unwrap_or_default().to_string(),
        created: fields.next().unwrap_or_default().to_string(),
        note: fields.next().unwrap_or_default().to_string(),
    }
}

pub(crate) fn field(line: &str, index: usize) -> Option<&str> {
    line.split('\t').nth(index)
}

pub(crate) fn selected_line_number(selection: &str) -> Result<usize> {
    field(selection, 0)
        .ok_or("waystone: selected row is malformed")?
        .parse::<usize>()
        .map_err(|_| "waystone: selected row has invalid line number".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tsv_record() {
        assert_eq!(
            parse_record("Label\t/path\tcreated\tnote with spaces"),
            Record {
                label: "Label".to_string(),
                path: "/path".to_string(),
                created: "created".to_string(),
                note: "note with spaces".to_string(),
            }
        );
    }

    #[test]
    fn sanitizes_tsv_fields() {
        assert_eq!(tsv_field("a\tb\nc\r"), "a b c ");
    }
}
