//! Model families. OpenAI names its models `gpt-<version>-<family>`
//! ("gpt-6.1-sol"), and a setting can name a family instead of a model, the
//! way `sonnet` means the newest Sonnet on the Claude CLI: "luna" is the
//! newest Luna on the list, whatever its version. The list is Codex's
//! catalog for a ChatGPT plan, or the models an OpenAI API key can use.

/// A model on a list, by its id.
pub trait ModelId {
    fn model_id(&self) -> &str;
}

impl ModelId for String {
    fn model_id(&self) -> &str {
        self
    }
}

/// The family and version an id names, when it has the
/// `gpt-<version>-<family>` shape: "gpt-6.1-sol" is ("sol", [6, 1]).
pub fn family_of(id: &str) -> Option<(&str, Vec<u32>)> {
    let (version, family) = id.strip_prefix("gpt-")?.split_once('-')?;
    let version: Vec<u32> = version.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    (!family.is_empty() && family.chars().all(|c| c.is_ascii_lowercase())).then_some((family, version))
}

/// The newest model of `family` in `models`.
pub fn newest<'a, M: ModelId>(models: &'a [M], family: &str) -> Option<&'a M> {
    models
        .iter()
        .filter_map(|m| family_of(m.model_id()).filter(|(f, _)| *f == family).map(|(_, v)| (v, m)))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, m)| m)
}

/// The families in `models`, in the list's order, each with its newest model.
pub fn families<M: ModelId>(models: &[M]) -> Vec<(&str, &M)> {
    let mut out: Vec<(&str, &M)> = vec![];
    for (family, _) in models.iter().filter_map(|m| family_of(m.model_id())) {
        if !out.iter().any(|(f, _)| *f == family)
            && let Some(m) = newest(models, family)
        {
            out.push((family, m));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A family is the newest of its models in the catalog, by version
    /// number: these are the models codex-cli 0.159.2 listed on 2026-09-30.
    #[test]
    fn a_family_means_its_newest_model() {
        let listed = crate::llm::codex_cli::built_in_models();
        let got: Vec<(&str, &str)> = families(&listed).into_iter().map(|(f, m)| (f, m.id.as_str())).collect();
        assert_eq!(got, [("sol", "gpt-6.1-sol"), ("astra", "gpt-6-astra"), ("luna", "gpt-6-luna"), ("terra", "gpt-5.6-terra")]);
        assert_eq!(family_of("gpt-6.1-sol"), Some(("sol", vec![6, 1])));
        assert_eq!(family_of("gpt-5.5"), None, "a model without a family name");
        assert_eq!(newest(&listed, "nova").map(|m| m.id.as_str()), None);
    }
}
