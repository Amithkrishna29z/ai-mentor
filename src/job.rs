//! Turning a job posting into a study track: matching the skills a posting
//! asks for onto subjects this app can teach. Pure string work - no CLI, no db.

/// Short forms and product names a posting uses that are not the catalogue's
/// own spelling. Left side is already normalised.
const ALIASES: &[(&str, &str)] = &[
    ("k8s", "Kubernetes"),
    ("kubernetes", "Kubernetes"),
    ("reactjs", "React"),
    ("react js", "React"),
    ("nodejs", "Node.js"),
    ("node", "Node.js"),
    ("postgres", "PostgreSQL"),
    ("psql", "PostgreSQL"),
    ("mongo", "MongoDB"),
    ("spring", "Spring Boot"),
    ("springboot", "Spring Boot"),
    ("spring mvc", "Spring Boot"),
    ("jpa", "Hibernate / JPA"),
    ("hibernate", "Hibernate / JPA"),
    ("orm", "Hibernate / JPA"),
    ("ci cd", "CI/CD (GitHub Actions)"),
    ("cicd", "CI/CD (GitHub Actions)"),
    ("github actions", "CI/CD (GitHub Actions)"),
    ("gitlab ci", "CI/CD (GitHub Actions)"),
    ("jenkins", "CI/CD (GitHub Actions)"),
    ("dsa", "Data Structures & Algorithms"),
    ("data structures", "Data Structures & Algorithms"),
    ("algorithms", "Data Structures & Algorithms"),
    ("problem solving", "Data Structures & Algorithms"),
    ("html", "HTML & CSS"),
    ("css", "HTML & CSS"),
    ("scss", "HTML & CSS"),
    ("ts", "TypeScript"),
    ("js", "JavaScript"),
    ("es6", "JavaScript"),
    ("tailwind", "Tailwind CSS"),
    ("dotnet", ".NET / C#"),
    ("net", ".NET / C#"),
    ("c#", ".NET / C#"),
    ("csharp", ".NET / C#"),
    ("amazon web services", "AWS"),
    ("google cloud", "GCP"),
    ("google cloud platform", "GCP"),
    ("microsoft azure", "Azure"),
    ("rest", "Spring Boot"),
    ("microservices", "System Design"),
    ("system design", "System Design"),
    ("distributed systems", "System Design"),
];

/// Lowercase, strip punctuation, and drop bare version numbers so
/// "Spring Boot 3.x" and "spring-boot" both land on "spring boot".
fn normalise(name: &str) -> String {
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '+' || c == '#' {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split_whitespace()
        // "java 17" -> "java"; "x" from "3.x" goes too.
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()) && *w != "x")
        .collect::<Vec<_>>()
        .join(" ")
}

/// Does `needle` appear in `hay` as a run of whole words? Word-wise rather
/// than substring, so "java" never matches inside "javascript".
fn has_words(hay: &str, needle: &str) -> bool {
    let hay: Vec<&str> = hay.split_whitespace().collect();
    let needle: Vec<&str> = needle.split_whitespace().collect();
    !needle.is_empty() && needle.len() <= hay.len() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Match one skill from a posting onto a subject the app already knows.
/// `known` is the catalogue. `None` means it should become a custom subject.
pub fn match_subject(skill: &str, known: &[String]) -> Option<String> {
    let want = normalise(skill);
    if want.is_empty() {
        return None;
    }
    let find = |name: &str| known.iter().find(|k| k.as_str() == name).cloned();

    // 1. The catalogue's own spelling.
    if let Some(hit) = known.iter().find(|k| normalise(k) == want) {
        return Some(hit.clone());
    }
    // 2. A known short form, exactly.
    for (alias, target) in ALIASES {
        if want == *alias {
            if let Some(hit) = find(target) {
                return Some(hit);
            }
        }
    }
    // 3. A catalogue subject named inside a longer phrase
    //    ("strong Spring Boot experience"). Longest name wins.
    let mut named: Vec<&String> = known
        .iter()
        .filter(|k| {
            let n = normalise(k);
            !n.is_empty() && has_words(&want, &n)
        })
        .collect();
    named.sort_by_key(|k| std::cmp::Reverse(normalise(k).split_whitespace().count()));
    if let Some(hit) = named.first() {
        return Some((*hit).clone());
    }
    // 4. A short form inside a longer phrase ("experience with k8s").
    let mut aliased: Vec<(&str, &str)> = ALIASES
        .iter()
        .filter(|(alias, _)| has_words(&want, alias))
        .copied()
        .collect();
    aliased.sort_by_key(|(alias, _)| std::cmp::Reverse(alias.split_whitespace().count()));
    for (_, target) in aliased {
        if let Some(hit) = find(target) {
            return Some(hit);
        }
    }
    None
}

/// Tidy a skill the catalogue does not cover into a subject name worth
/// studying, so a custom subject does not end up called "strong grpc skills".
pub fn custom_subject_name(skill: &str) -> String {
    let trimmed = skill.trim();
    let mut out = String::new();
    for (i, word) in trimmed.split_whitespace().take(4).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.is_empty() {
        return String::new();
    }
    // A deliberate capital anywhere in the first word means the name styles
    // itself (gRPC, GraphQL) - leave it. Only plain prose gets lifted.
    let styles_itself = out
        .split_whitespace()
        .next()
        .is_some_and(|w| w.chars().any(|c| c.is_uppercase()));
    if styles_itself {
        return out;
    }
    let mut chars = out.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogue() -> Vec<String> {
        crate::models::CATALOGUE
            .iter()
            .flat_map(|(_, names)| names.iter().map(|n| (*n).to_string()))
            .collect()
    }

    #[test]
    fn a_version_number_does_not_stop_a_match() {
        let c = catalogue();
        assert_eq!(match_subject("Java 17", &c).as_deref(), Some("Java"));
        assert_eq!(
            match_subject("Spring Boot 3.x", &c).as_deref(),
            Some("Spring Boot")
        );
        assert_eq!(match_subject("spring-boot", &c).as_deref(), Some("Spring Boot"));
    }

    #[test]
    fn java_never_matches_javascript() {
        let c = catalogue();
        assert_eq!(match_subject("Java", &c).as_deref(), Some("Java"));
        assert_eq!(match_subject("JavaScript", &c).as_deref(), Some("JavaScript"));
        assert_eq!(
            match_subject("Strong JavaScript fundamentals", &c).as_deref(),
            Some("JavaScript"),
            "the longer phrase still resolves to JavaScript, not Java"
        );
    }

    #[test]
    fn short_forms_resolve_to_the_catalogue_spelling() {
        let c = catalogue();
        assert_eq!(match_subject("K8s", &c).as_deref(), Some("Kubernetes"));
        assert_eq!(match_subject("Postgres", &c).as_deref(), Some("PostgreSQL"));
        assert_eq!(
            match_subject("CI/CD", &c).as_deref(),
            Some("CI/CD (GitHub Actions)")
        );
        assert_eq!(
            match_subject("DSA", &c).as_deref(),
            Some("Data Structures & Algorithms")
        );
    }

    #[test]
    fn a_skill_named_inside_a_sentence_is_found() {
        let c = catalogue();
        assert_eq!(
            match_subject("Hands-on experience with Docker", &c).as_deref(),
            Some("Docker")
        );
        assert_eq!(
            match_subject("deploying to AWS at scale", &c).as_deref(),
            Some("AWS")
        );
        assert_eq!(
            match_subject("comfortable with k8s in production", &c).as_deref(),
            Some("Kubernetes")
        );
    }

    #[test]
    fn something_the_catalogue_cannot_teach_returns_none() {
        let c = catalogue();
        assert_eq!(match_subject("gRPC", &c), None);
        assert_eq!(match_subject("", &c), None);
        assert_eq!(match_subject("   ", &c), None);
    }

    #[test]
    fn custom_subjects_keep_their_own_capitalisation_and_stay_short() {
        assert_eq!(custom_subject_name("gRPC"), "gRPC");
        assert_eq!(custom_subject_name("GraphQL"), "GraphQL");
        assert_eq!(custom_subject_name("  rabbitmq  "), "Rabbitmq");
        assert_eq!(
            custom_subject_name("event driven architecture with brokers and queues"),
            "Event driven architecture with",
            "a long phrase is trimmed to something studiable"
        );
    }
}
