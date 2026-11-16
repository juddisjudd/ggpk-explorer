

use serde::Serialize;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CsdFile {
    pub path: String,
    pub entries: Vec<CsdEntry>,
    pub languages: Vec<String>,
    /// Other `.csd` files this one pulls in with `include "…"`.
    pub includes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CsdEntry {
    pub ids: Vec<String>,
    pub descriptions: Vec<CsdSubEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CsdSubEntry {
    pub operator: String,
    pub description: String,
    pub is_canonical: bool,
    pub parameters: Vec<CsdParameter>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CsdParameter {
    pub name: String,
    /// 1-based index of the value the handler applies to; zero when the
    /// handler takes a name instead (`reminderstring`).
    pub value: i32,
    /// Further value indexes the handler reads but does not format, as in `alternate_tree_version_keystone 3 1 4`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<i32>,
    /// The name a non-numeric handler was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

impl fmt::Display for CsdParameter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.name)?;
        if let Some(text) = &self.text {
            return write!(f, " {text}");
        }
        if self.value != 0 {
            write!(f, " {}", self.value)?;
        }
        self.inputs.iter().try_for_each(|input| write!(f, " {input}"))
    }
}

pub fn parse_csd(data: &[u8], file_path: &str) -> Result<CsdFile, String> {
    // 1. Decode UTF-16LE
    let u16_vec: Vec<u16> = data
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    let content = String::from_utf16(&u16_vec)
        .map_err(|e| format!("Failed to decode UTF-16LE: {}", e))?;
    // The byte-order mark would otherwise glue itself to the first keyword.
    let content = content.strip_prefix('\u{feff}').unwrap_or(&content);

    let mut entries = Vec::new();
    let mut includes = Vec::new();
    let mut languages = HashSet::new();
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("include") {
            if let Some(path) = line.split('"').nth(1) {
                includes.push(path.to_string());
            }
            i += 1;
            continue;
        }
        if line.starts_with('\t') {
            i += 1;
            continue;
        }

        if line.starts_with("no_description") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 1 {
                entries.push(CsdEntry {
                    ids: vec![parts[1].to_string()],
                    descriptions: Vec::new(),
                });
            }
            i += 1;
            continue;
        }

        if line.starts_with("description") {
            let mut current_ids = Vec::new();
            
            // "description" line logic from C# reference:
            // "description id1 id2 ..." or "description" then next line has params?
            // The C# logic says:
            // if line starts with description:
            //   i++
            //   read next line -> split by space -> first part is count?
            // Let's re-read the C# logic carefully.
            
            // C# Logic RE-ANALYSIS:
            // if (line.StartsWith("description")) {
            //    i++; line = lines[i]; 
            //    parts = line.split()
            //    partsCount = parts[0]
            //    ... read IDs ...
            //    i++; count = int.Parse(lines[i]) -> number of sub entries
            // }

            // So "description" is just a marker. The NEXT line contains content.
            i += 1;
            if i >= lines.len() { break; }
            let id_line = lines[i];
            let id_parts: Vec<&str> = id_line.split_whitespace().collect();
            
            if let Some(count_str) = id_parts.get(0) {
                 if let Ok(id_count) = count_str.parse::<usize>() {
                     // Check range constraints from C# (count <= 0 or >= 5 continue??)
                     // "if (partsCount is <= 0 or >= 5) continue;"
                     if id_count > 0 {
                         for part in id_parts.iter().skip(1).take(id_count) {
                             current_ids.push(part.to_string());
                         }
                     }
                 }
            }

            i += 1; // Move to count of descriptions
            if i >= lines.len() { break; }

            let mut descriptions = Vec::new();
            let mut current_lang: Option<String> = None;
            let mut has_base_desc = false;
            
            // Loop until we hit a new keyword or EOF
            loop {
                // Peek next line check?
                if i + 1 >= lines.len() { break; }
                
                let next_line = lines[i+1].trim();
                if next_line.starts_with("description") || next_line.starts_with("no_description") || next_line.starts_with("include") {
                    break;
                }
                
                i += 1;
                let line = lines[i].trim();

                // Handle language switch
                if line.starts_with("lang ") {
                    let parts: Vec<&str> = line.split('"').collect();
                    if parts.len() >= 2 {
                        let lang = parts[1].to_string();
                        languages.insert(lang.clone());
                        current_lang = Some(lang);
                    }
                    continue; 
                }

                // Parse: Operator "Description" [Params...]
                let parts: Vec<&str> = line.split('"').collect();
                if parts.len() >= 2 {
                    let operator = parts[0].trim().to_string();
                    let description = parts[1].replace("\\n", "\n");
                    
                    let mut is_canonical = false;
                    let mut parameters = Vec::new();

                    if parts.len() > 2 {
                        let param_str = parts[2..].join("\""); // Rejoin rest
                        let param_parts: Vec<&str> = param_str.split_whitespace().collect();
                        
                        let mut p_idx = 0;
                        while p_idx < param_parts.len() {
                            let name = param_parts[p_idx];
                            p_idx += 1;
                            if name == "canonical_line" {
                                is_canonical = true;
                                continue;
                            }
                            let mut indexes = Vec::new();
                            while let Some(Ok(index)) = param_parts.get(p_idx).map(|t| t.parse::<i32>()) {
                                indexes.push(index);
                                p_idx += 1;
                            }
                            let mut param = CsdParameter { name: name.to_string(), value: 0, inputs: Vec::new(), text: None };
                            if let Some((&first, rest)) = indexes.split_first() {
                                param.value = first;
                                param.inputs = rest.to_vec();
                            } else if let Some(arg) = param_parts.get(p_idx) {
                                // A handler whose argument names something (`reminderstring ReminderTextX`).
                                param.text = Some(arg.to_string());
                                p_idx += 1;
                            }
                            parameters.push(param);
                        }
                    }

                    descriptions.push(CsdSubEntry {
                        operator,
                        description,
                        is_canonical,
                        parameters,
                        language: current_lang.clone(),
                    });
                    
                    if current_lang.is_none() {
                        has_base_desc = true;
                    }
                }

            }

            
            entries.push(CsdEntry {
                ids: current_ids,
                descriptions,
            });

            if has_base_desc {
                languages.insert("English".to_string());
            }
            
            i += 1;
            continue;
        }

        i += 1;
    }

    Ok(CsdFile {
        path: file_path.to_string(),
        entries,
        includes,
        languages: {
            let mut l: Vec<_> = languages.into_iter().collect();
            l.sort();
            l
        }, // sorted list of languages
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csd_parse() {
        // Construct a mock UTF-16LE buffer with localization
        let text = "description
2 id1 id2
1
1 \"Text with \\n newline\" canonical_line param1 100
lang \"Traditional Chinese\"
1 \"Chinese Text\"
lang \"Japanese\"
1 \"Japanese Text\"";
        
        let u16_vec: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = Vec::new();
        for u in u16_vec {
            bytes.extend_from_slice(&u.to_le_bytes());
        }

        let result = parse_csd(&bytes, "test.csd").expect("Failed to parse");
        assert_eq!(result.entries.len(), 1);
        let entry = &result.entries[0];
        assert_eq!(entry.ids.len(), 2);
        
        // Should capture English + Chinese + Japanese = 3 descriptions
        assert_eq!(entry.descriptions.len(), 3);
        
        assert_eq!(entry.descriptions[0].description, "Text with \n newline");
        assert!(entry.descriptions[0].language.is_none());
        
        assert_eq!(entry.descriptions[1].description, "Chinese Text");
        assert_eq!(entry.descriptions[1].language.as_deref(), Some("Traditional Chinese"));
        
        assert_eq!(entry.descriptions[2].description, "Japanese Text");
        assert_eq!(entry.descriptions[2].language.as_deref(), Some("Japanese"));
        
        assert_eq!(result.languages.len(), 3);
        assert!(result.languages.contains(&"Traditional Chinese".to_string()));
        assert!(result.languages.contains(&"Japanese".to_string()));
        assert!(result.languages.contains(&"English".to_string()));
    }

    #[test]
    fn handlers_take_several_value_indexes() {
        let text = "description
4 version seed keystone revision
1
1 # # # \"{1} sacrificed in the name of {2}\\nConquered by the {0}\" alternate_tree_version_name 1 alternate_tree_version_keystone 3 1 4 reminderstring ReminderTextConqueredPassives
description
3 slot gem level
1
# # # \"Skills granted by your {0} are Supported by level {2} {1}\" pelagic_pearl_slot 1 display_indexable_non_active_support 2 3";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let result = parse_csd(&bytes, "test.csd").expect("Failed to parse");
        let params = |entry: usize| -> Vec<String> {
            result.entries[entry].descriptions[0].parameters.iter().map(|p| p.to_string()).collect()
        };

        assert_eq!(
            params(0),
            ["alternate_tree_version_name 1", "alternate_tree_version_keystone 3 1 4", "reminderstring ReminderTextConqueredPassives"]
        );
        assert_eq!(params(1), ["pelagic_pearl_slot 1", "display_indexable_non_active_support 2 3"]);
        let keystone = &result.entries[0].descriptions[0].parameters[1];
        assert_eq!((keystone.value, keystone.inputs.as_slice()), (3, [1, 4].as_slice()));
    }
}
