use super::fuzzy_score;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Home,
    Measure,
    System,
    Learn,
    Customize,
    Bloat,
    Models,
    HuggingFace,
    Serve,
    Connect,
    Recipes,
    Benchmark,
    Sweep,
    Publish,
    LocalAi,
    LocalAiRefresh,
    Panels,
    Walkthrough,
}

impl Action {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Home => "view.overview",
            Self::Measure => "view.measure",
            Self::System => "view.system",
            Self::Learn => "view.learn",
            Self::Customize => "view.setup",
            Self::Bloat => "view.bloat",
            Self::Models => "models.open",
            Self::HuggingFace => "models.huggingface",
            Self::Serve => "serve.toggle",
            Self::Connect => "connect.open",
            Self::Recipes => "benchmark.recipes",
            Self::Benchmark => "benchmark.quick",
            Self::Sweep => "benchmark.sweep",
            Self::Publish => "publish.preview",
            Self::LocalAi => "recommendations.local_ai.view",
            Self::LocalAiRefresh => "recommendations.local_ai.refresh",
            Self::Panels => "layout.panels",
            Self::Walkthrough => "onboarding.open",
        }
    }
}

pub struct Item {
    pub key: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub action: Action,
}

pub fn catalog() -> Vec<Item> {
    vec![
        Item {
            key: "1",
            label: "Overview",
            detail: "device capacity, active model, and next action",
            action: Action::Home,
        },
        Item {
            key: "2",
            label: "Measure",
            detail: "rates, requests, stages, and benchmark results",
            action: Action::Measure,
        },
        Item {
            key: "3",
            label: "System",
            detail: "memory, endpoints, and pressure",
            action: Action::System,
        },
        Item {
            key: "4",
            label: "Learn",
            detail: "plain explanations using current readings",
            action: Action::Learn,
        },
        Item {
            key: "5",
            label: "Setup",
            detail: "theme, density, home screen, and panels",
            action: Action::Customize,
        },
        Item {
            key: "6",
            label: "Bloat",
            detail: "scan results, evidence, and guarded cleanup",
            action: Action::Bloat,
        },
        Item {
            key: "m",
            label: "Choose model",
            detail: "load a local target or inspect runtimes",
            action: Action::Models,
        },
        Item {
            key: "h",
            label: "Hugging Face models",
            detail: "check and download pinned public safetensors",
            action: Action::HuggingFace,
        },
        Item {
            key: "s",
            label: "Start or stop serving",
            detail: "toggle the configured local model server",
            action: Action::Serve,
        },
        Item {
            key: "c",
            label: "Connect agent",
            detail: "configure an agent detected on this device",
            action: Action::Connect,
        },
        Item {
            key: "r",
            label: "Run recipe",
            detail: "choose a workload-shaped benchmark",
            action: Action::Recipes,
        },
        Item {
            key: "b",
            label: "Run quick benchmark",
            detail: "measure a short local baseline",
            action: Action::Benchmark,
        },
        Item {
            key: "B",
            label: "Run prompt sweep",
            detail: "measure prefill across context sizes",
            action: Action::Sweep,
        },
        Item {
            key: "p",
            label: "Preview result",
            detail: "redact before copying or saving",
            action: Action::Publish,
        },
        Item {
            key: "l",
            label: "View local.ai source",
            detail: "inspect cached public recommendations and provenance",
            action: Action::LocalAi,
        },
        Item {
            key: "L",
            label: "Refresh local.ai",
            detail: "refresh through an available public-web search adapter",
            action: Action::LocalAiRefresh,
        },
        Item {
            key: "P",
            label: "Customize panels",
            detail: "choose what appears in detailed views",
            action: Action::Panels,
        },
        Item {
            key: "tour",
            label: "Quick walkthrough",
            detail: "choose a goal and open the right starting view",
            action: Action::Walkthrough,
        },
    ]
}

pub fn matches(query: &str) -> Vec<usize> {
    let items = catalog();
    let mut matches = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let haystack = format!(
                "{} {} {} {}",
                item.key,
                item.action.id(),
                item.label,
                item.detail
            );
            fuzzy_score(query, &haystack).map(|score| (index, score))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    matches.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_and_human_commands_share_stable_ids() {
        let items = catalog();
        assert!(items.iter().any(|item| item.action.id() == "view.bloat"));
        assert!(items
            .iter()
            .any(|item| item.key == "m" && item.action.id() == "models.open"));
        assert!(items
            .iter()
            .any(|item| item.key == "s" && item.action.id() == "serve.toggle"));
        assert!(items
            .iter()
            .any(|item| item.key == "tour" && item.action.id() == "onboarding.open"));
    }

    #[test]
    fn fuzzy_command_search_matches_stable_ids() {
        let items = catalog();
        let matches = matches("benchmark.quick");
        assert_eq!(items[matches[0]].action, Action::Benchmark);
    }
}
