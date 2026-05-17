#![allow(dead_code)]
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub content: String,
    pub path: PathBuf,
}

/// Scan ~/.cc-gateway/skills/ and current project's .claude/skills/ for skill files.
pub fn scan_skills(additional_dirs: &[String]) -> Vec<Skill> {
    let mut skills = Vec::new();

    // User-level skills
    if let Some(home) = dirs::home_dir() {
        let user_skills_dir = home.join(".cc-gateway").join("skills");
        skills.extend(scan_skill_dir(&user_skills_dir));
    }

    // Project-level skills from additional dirs
    for dir in additional_dirs {
        let expanded = shellexpand::tilde(dir).to_string();
        let project_skills = PathBuf::from(&expanded).join(".claude").join("skills");
        skills.extend(scan_skill_dir(&project_skills));
    }

    skills
}

fn scan_skill_dir(dir: &PathBuf) -> Vec<Skill> {
    let mut skills = Vec::new();
    if !dir.exists() || !dir.is_dir() {
        return skills;
    }

    let re = Regex::new(r"^---\s*\n(.*?)\n---\s*\n(.*)$").unwrap();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            match fs::read_to_string(&path) {
                Ok(content) => {
                    let (frontmatter, body) = if let Some(caps) = re.captures(&content) {
                        let fm = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                        let body = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                        (fm, body)
                    } else {
                        ("", content.as_str())
                    };

                    let description = parse_frontmatter_field(frontmatter, "description");
                    skills.push(Skill {
                        name,
                        description,
                        content: body.trim().to_string(),
                        path,
                    });
                    debug!("Loaded skill: {}", skills.last().unwrap().name);
                }
                Err(e) => {
                    warn!("Failed to read skill {}: {}", path.display(), e);
                }
            }
        }
    }

    skills
}

fn parse_frontmatter_field(frontmatter: &str, field: &str) -> String {
    let re = Regex::new(&format!(r"(?m)^{}:\s*(.*)$", regex::escape(field))).unwrap();
    re.captures(frontmatter)
        .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
        .unwrap_or_default()
}

pub fn build_skill_system_prompt(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }

    let mut prompt = String::from("You have access to the following skills:\n\n");
    for skill in skills {
        prompt.push_str(&format!("## Skill: {}\n", skill.name));
        if !skill.description.is_empty() {
            prompt.push_str(&format!("Description: {}\n", skill.description));
        }
        prompt.push_str(&format!("{}\n\n", skill.content));
    }
    prompt
}

pub fn find_skill<'a>(skills: &'a [Skill], name: &str) -> Option<&'a Skill> {
    skills.iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_field() {
        let fm = "name: weather\ndescription: Get weather info\n";
        assert_eq!(parse_frontmatter_field(fm, "name"), "weather");
        assert_eq!(parse_frontmatter_field(fm, "description"), "Get weather info");
        assert_eq!(parse_frontmatter_field(fm, "missing"), "");
    }

    #[test]
    fn test_build_skill_system_prompt_empty() {
        let skills: Vec<Skill> = vec![];
        assert_eq!(build_skill_system_prompt(&skills), "");
    }

    #[test]
    fn test_build_skill_system_prompt() {
        let skills = vec![Skill {
            name: "test".to_string(),
            description: "A test skill".to_string(),
            content: "Do testing.".to_string(),
            path: PathBuf::from("/tmp/test.md"),
        }];
        let prompt = build_skill_system_prompt(&skills);
        assert!(prompt.contains("test"));
        assert!(prompt.contains("Do testing."));
    }

    #[test]
    fn test_find_skill() {
        let skills = vec![
            Skill {
                name: "a".to_string(),
                description: "".to_string(),
                content: "".to_string(),
                path: PathBuf::from("/tmp/a.md"),
            },
            Skill {
                name: "b".to_string(),
                description: "".to_string(),
                content: "".to_string(),
                path: PathBuf::from("/tmp/b.md"),
            },
        ];
        assert!(find_skill(&skills, "a").is_some());
        assert!(find_skill(&skills, "c").is_none());
    }
}
