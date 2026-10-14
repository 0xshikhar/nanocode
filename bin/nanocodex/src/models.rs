use nanocode::{
    HarnessFamily, ModelSelection, ModelTransport, ReasoningMode, ServiceTier, Thinking,
};
use nanocode_tui::picker::{ItemKind, Picker, PickerItem};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TierOption {
    Tier(ServiceTier),
    Pro,
}

pub fn catalog() -> Vec<(HarnessFamily, Vec<ModelSelection>)> {
    HarnessFamily::builtin()
        .into_iter()
        .map(|family| {
            let models = family.models().collect();
            (family, models)
        })
        .collect()
}

fn harness_title(family: &HarnessFamily) -> String {
    if *family == HarnessFamily::Claude {
        "Claude".into()
    } else {
        "Codex".into()
    }
}

pub fn features(model: &ModelSelection) -> String {
    let capabilities = model.capabilities(ModelTransport::Native);
    let mut features = Vec::new();
    for tier in capabilities
        .service_tiers()
        .filter(|tier| *tier != ServiceTier::Standard)
    {
        features.push(tier.as_str());
    }
    if capabilities.supports_reasoning_mode(ReasoningMode::Pro) {
        features.push("pro");
    }
    features.join(" · ")
}

pub fn model_picker(current: &ModelSelection) -> Picker {
    let mut items = Vec::new();
    for (family, models) in catalog() {
        items.push(PickerItem::header(harness_title(&family)));
        for model in models {
            let capabilities = model.capabilities(ModelTransport::Native);
            let thinking = capabilities
                .thinking()
                .map(Thinking::as_str)
                .collect::<Vec<_>>()
                .join("/");
            let mut item = PickerItem::choice(model.to_string())
                .detail(format!("{} · thinking {thinking}", model.display_name()))
                .current(model == *current);
            let features = features(&model);
            if !features.is_empty() {
                item = item.badge(features);
            }
            items.push(item);
        }
    }
    Picker::new("Model", items)
        .filterable()
        .footer("type to filter · enter select · esc cancel")
}

pub fn model_at(picker: &Picker, index: usize) -> Option<ModelSelection> {
    let item = picker.items().get(index)?;
    (item.kind == ItemKind::Choice).then(|| crate::host::parse_model(&item.label).ok())?
}

pub fn thinking_picker(model: &ModelSelection, current: Thinking) -> Picker {
    let capabilities = model.capabilities(ModelTransport::Native);
    let items = capabilities
        .thinking()
        .map(|thinking| {
            let mut item = PickerItem::choice(thinking.as_str()).current(thinking == current);
            if thinking == capabilities.default_thinking() {
                item = item.detail("default");
            }
            item
        })
        .collect();
    Picker::new(format!("Thinking · {model}"), items).footer("enter select · esc cancel")
}

pub fn thinking_at(model: &ModelSelection, picker: &Picker, index: usize) -> Option<Thinking> {
    let label = &picker.items().get(index)?.label;
    model
        .capabilities(ModelTransport::Native)
        .thinking()
        .find(|thinking| thinking.as_str() == label)
}

pub fn tier_options(model: &ModelSelection) -> Vec<TierOption> {
    let capabilities = model.capabilities(ModelTransport::Native);
    let mut options = capabilities
        .service_tiers()
        .map(TierOption::Tier)
        .collect::<Vec<_>>();
    if capabilities.supports_reasoning_mode(ReasoningMode::Pro) {
        options.push(TierOption::Pro);
    }
    options
}

pub fn tier_picker(
    model: &ModelSelection,
    options: &[TierOption],
    current: ServiceTier,
    pro: bool,
) -> Picker {
    let items = options
        .iter()
        .map(|option| match option {
            TierOption::Tier(tier) => PickerItem::choice(tier.as_str())
                .detail(match tier {
                    ServiceTier::Standard => "default speed",
                    ServiceTier::Fast | ServiceTier::Priority => "priority processing",
                    ServiceTier::Ultrafast => "lowest latency",
                })
                .current(*tier == current && !pro),
            TierOption::Pro => PickerItem::choice("pro")
                .detail("pro reasoning mode (reopens the session)")
                .current(pro),
        })
        .collect();
    Picker::new(format!("Speed · {model}"), items).footer("enter select · esc cancel")
}
