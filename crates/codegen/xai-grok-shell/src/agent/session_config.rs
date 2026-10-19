use agent_client_protocol as acp;
use serde::Serialize;
use xai_grok_sampling_types::{ReasoningEffort, ReasoningEffortOption, effort_label};

use crate::session::unified_list::SessionKind;

pub(crate) const SELECTABLE_REASONING_EFFORTS: [ReasoningEffort; 5] = [
    ReasoningEffort::Minimal,
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
];

pub(crate) const CONFIG_ID_MODEL: &str = "model";
pub(crate) const CONFIG_ID_REASONING_EFFORT: &str = "reasoning_effort";
pub(crate) const CONFIG_ID_PERMISSION_MODE: &str = "permission_mode";

pub(crate) const PERMISSION_MODE_ASK: &str = "ask";
pub(crate) const PERMISSION_MODE_AUTO: &str = "auto";
pub(crate) const PERMISSION_MODE_YOLO: &str = "always-approve";

/// Live permission posture advertised as an ACP `category: mode` select.
///
/// `acceptEdits` is not a runtime switch (it is a startup rule), so it is not a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PermissionModeView {
    pub yolo: bool,
    pub auto: bool,
    /// When false, the Auto row is omitted unless the session is already in auto.
    pub auto_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionConfigOption {
    pub id: String,
    pub category: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GrokSessionDetail {
    pub session_id: String,
    pub kind: String,
    pub cwd: String,
    pub current_model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl GrokSessionDetail {
    pub(crate) fn build(
        session_id: String,
        cwd: String,
        current_model_id: String,
        title: Option<String>,
    ) -> Self {
        Self {
            session_id,
            kind: SessionKind::Build.as_ref().to_string(),
            cwd,
            current_model_id,
            title,
        }
    }
}

/// The built-in session-picker modes used when the model has no server list.
/// Reproduces the historical five rows and their labels.
pub(crate) fn legacy_session_effort_options() -> Vec<ReasoningEffortOption> {
    SELECTABLE_REASONING_EFFORTS
        .iter()
        .map(|&effort| ReasoningEffortOption {
            id: effort.as_ref().to_string(),
            value: effort,
            label: effort_label(effort),
            description: None,
            default: false,
        })
        .collect()
}

fn model_display_name(model: &acp::ModelInfo) -> String {
    if model.name.is_empty() {
        model.model_id.0.to_string()
    } else {
        model.name.clone()
    }
}

pub(crate) fn build_session_config_options(
    available_models: &[acp::ModelInfo],
    current_model_id: &acp::ModelId,
    effort_options: &[ReasoningEffortOption],
    current_effort: Option<ReasoningEffort>,
) -> Vec<SessionConfigOption> {
    let mut options = Vec::with_capacity(available_models.len() + effort_options.len());

    for model in available_models {
        options.push(SessionConfigOption {
            id: model.model_id.0.to_string(),
            category: "model".to_string(),
            label: model_display_name(model),
            description: None,
            selected: model.model_id == *current_model_id,
        });
    }

    for effort in effort_options {
        options.push(SessionConfigOption {
            id: effort.id.clone(),
            category: "mode".to_string(),
            label: effort.label.clone(),
            description: effort.description.clone(),
            selected: Some(effort.value) == current_effort,
        });
    }

    options
}

/// Current select id. Always-approve wins over auto, matching the permission manager.
pub(crate) fn permission_mode_current(view: PermissionModeView) -> &'static str {
    if view.yolo {
        PERMISSION_MODE_YOLO
    } else if view.auto {
        PERMISSION_MODE_AUTO
    } else {
        PERMISSION_MODE_ASK
    }
}

fn permission_mode_option(view: PermissionModeView) -> acp::SessionConfigOption {
    let mut values = vec![acp::SessionConfigSelectOption::new(PERMISSION_MODE_ASK, "Default")
        .description("Prompt before tools that change something")];
    if view.auto_available || view.auto {
        values.push(
            acp::SessionConfigSelectOption::new(PERMISSION_MODE_AUTO, "Auto").description(
                "Classifier approves calls it judges safe. Dangerous calls may still prompt",
            ),
        );
    }
    values.push(
        acp::SessionConfigSelectOption::new(PERMISSION_MODE_YOLO, "YOLO").description(
            "Run tools without permission prompts. Deny rules and hooks still apply",
        ),
    );
    acp::SessionConfigOption::select(
        CONFIG_ID_PERMISSION_MODE,
        "Permission",
        permission_mode_current(view),
        values,
    )
    .category(acp::SessionConfigOptionCategory::Mode)
}

pub(crate) fn build_acp_config_options(
    available_models: &[acp::ModelInfo],
    current_model_id: &acp::ModelId,
    effort_options: &[ReasoningEffortOption],
    current_effort: Option<ReasoningEffort>,
    permission_mode: PermissionModeView,
) -> Vec<acp::SessionConfigOption> {
    let mut options = Vec::new();

    if !available_models.is_empty() {
        let values: Vec<acp::SessionConfigSelectOption> = available_models
            .iter()
            .map(|model| {
                acp::SessionConfigSelectOption::new(
                    model.model_id.0.to_string(),
                    model_display_name(model),
                )
            })
            .collect();
        // Keep the real model even if the catalog doesn't list it.
        let current_value = current_model_id.0.to_string();
        options.push(
            acp::SessionConfigOption::select(CONFIG_ID_MODEL, "Model", current_value, values)
                .category(acp::SessionConfigOptionCategory::Model),
        );
    }

    if !effort_options.is_empty() {
        // Keep the real effort even if it isn't a listed option (e.g. none/max);
        // fall back to the model default when none is set.
        let current_value = match current_effort {
            Some(effort) => effort_options
                .iter()
                .find(|option| option.value == effort)
                .map(|option| option.id.clone())
                .unwrap_or_else(|| effort.as_ref().to_string()),
            None => {
                let option = effort_options
                    .iter()
                    .find(|option| option.default)
                    .or(effort_options.first())
                    .expect("effort_options is non-empty");
                option.id.clone()
            }
        };
        let values: Vec<acp::SessionConfigSelectOption> = effort_options
            .iter()
            .map(|option| {
                let mut value =
                    acp::SessionConfigSelectOption::new(option.id.clone(), option.label.clone());
                if let Some(description) = &option.description {
                    value = value.description(description.clone());
                }
                value
            })
            .collect();
        options.push(
            acp::SessionConfigOption::select(
                CONFIG_ID_REASONING_EFFORT,
                "Reasoning Effort",
                current_value,
                values,
            )
            .category(acp::SessionConfigOptionCategory::ThoughtLevel),
        );
    }

    options.push(permission_mode_option(permission_mode));
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &'static str, name: &str) -> acp::ModelInfo {
        acp::ModelInfo::new(acp::ModelId::new(id), name.to_string())
    }

    #[test]
    fn options_have_one_selected_model_and_a_mode_per_effort() {
        let models = [
            model("grok-build", "Grok Build"),
            model("grok-4.5", "Grok 4.5"),
        ];
        let current = acp::ModelId::from("grok-build");
        let opts = build_session_config_options(
            &models,
            &current,
            &legacy_session_effort_options(),
            Some(ReasoningEffort::High),
        );

        let model_opts: Vec<_> = opts.iter().filter(|o| o.category == "model").collect();
        assert_eq!(model_opts.len(), 2);
        let selected_models: Vec<_> = model_opts.iter().filter(|o| o.selected).collect();
        assert_eq!(selected_models.len(), 1);
        let Some(selected_model) = selected_models.first() else {
            panic!("expected one selected model: {selected_models:?}");
        };
        assert_eq!(selected_model.id, "grok-build");

        let mode_opts: Vec<_> = opts.iter().filter(|o| o.category == "mode").collect();
        assert_eq!(mode_opts.len(), SELECTABLE_REASONING_EFFORTS.len());
        let selected_modes: Vec<_> = mode_opts.iter().filter(|o| o.selected).collect();
        assert_eq!(selected_modes.len(), 1);
        let Some(selected_mode) = selected_modes.first() else {
            panic!("expected one selected mode: {selected_modes:?}");
        };
        assert_eq!(selected_mode.id, "high");
        assert_eq!(selected_mode.label, "High");
    }

    #[test]
    fn none_effort_is_not_a_user_selectable_mode() {
        assert!(!SELECTABLE_REASONING_EFFORTS.contains(&ReasoningEffort::None));
        let models = [model("grok-build", "Grok Build")];
        let current = acp::ModelId::from("grok-build");
        let opts = build_session_config_options(
            &models,
            &current,
            &legacy_session_effort_options(),
            Some(ReasoningEffort::None),
        );
        let modes: Vec<_> = opts.iter().filter(|o| o.category == "mode").collect();
        assert!(modes.iter().all(|o| o.id != "none"));
        assert!(modes.iter().all(|o| !o.selected));
    }

    #[test]
    fn no_mode_options_when_model_lacks_effort_support() {
        let models = [model("grok-build", "Grok Build")];
        let current = acp::ModelId::from("grok-build");
        let opts = build_session_config_options(&models, &current, &[], None);
        assert_eq!(opts.len(), 1);
        assert!(opts.iter().all(|o| o.category == "model"));
    }

    #[test]
    fn model_label_falls_back_to_id_when_name_empty() {
        let models = [model("grok-build", "")];
        let current = acp::ModelId::from("grok-build");
        let opts = build_session_config_options(&models, &current, &[], None);
        let Some(first) = opts.first() else {
            panic!("expected one option: {opts:?}");
        };
        assert_eq!(first.label, "grok-build");
    }

    #[test]
    fn session_config_option_serializes_camel_case() {
        let opt = SessionConfigOption {
            id: "grok-build".to_string(),
            category: "model".to_string(),
            label: "Grok Build".to_string(),
            description: None,
            selected: true,
        };
        let v = serde_json::to_value(&opt).expect("serialize");
        assert_eq!(v.get("id").and_then(|x| x.as_str()), Some("grok-build"));
        assert_eq!(v.get("category").and_then(|x| x.as_str()), Some("model"));
        assert_eq!(v.get("label").and_then(|x| x.as_str()), Some("Grok Build"));
        assert_eq!(v.get("selected").and_then(|x| x.as_bool()), Some(true));
        assert!(v.get("description").is_none());
    }

    #[test]
    fn grok_session_detail_serializes_camel_case() {
        let detail = GrokSessionDetail::build(
            "sess-1".to_string(),
            "/Users/me/xai".to_string(),
            "grok-build".to_string(),
            None,
        );
        let v = serde_json::to_value(&detail).expect("serialize");
        assert_eq!(v.get("sessionId").and_then(|x| x.as_str()), Some("sess-1"));
        assert_eq!(v.get("kind").and_then(|x| x.as_str()), Some("build"));
        assert_eq!(v.get("cwd").and_then(|x| x.as_str()), Some("/Users/me/xai"));
        assert_eq!(
            v.get("currentModelId").and_then(|x| x.as_str()),
            Some("grok-build")
        );
        assert!(v.get("title").is_none());
    }

    #[test]
    fn acp_config_options_map_model_and_effort_selectors() {
        let models = [
            model("grok-build", "Grok Build"),
            model("grok-4.5", "Grok 4.5"),
        ];
        let efforts = [ReasoningEffortOption {
            id: "high".to_string(),
            value: ReasoningEffort::High,
            label: "High".to_string(),
            description: None,
            default: false,
        }];

        let permission = PermissionModeView {
            yolo: false,
            auto: false,
            auto_available: true,
        };
        let options = build_acp_config_options(
            &models,
            &acp::ModelId::from("grok-4.5"),
            &efforts,
            Some(ReasoningEffort::High),
            permission,
        );

        let expected = vec![
            acp::SessionConfigOption::select(
                CONFIG_ID_MODEL,
                "Model",
                "grok-4.5",
                vec![
                    acp::SessionConfigSelectOption::new("grok-build", "Grok Build"),
                    acp::SessionConfigSelectOption::new("grok-4.5", "Grok 4.5"),
                ],
            )
            .category(acp::SessionConfigOptionCategory::Model),
            acp::SessionConfigOption::select(
                CONFIG_ID_REASONING_EFFORT,
                "Reasoning Effort",
                "high",
                vec![acp::SessionConfigSelectOption::new("high", "High")],
            )
            .category(acp::SessionConfigOptionCategory::ThoughtLevel),
            permission_mode_option(permission),
        ];
        assert_eq!(options, expected);
    }

    #[test]
    fn permission_mode_current_prefers_yolo_and_hides_auto_when_unavailable() {
        assert_eq!(
            permission_mode_current(PermissionModeView {
                yolo: true,
                auto: true,
                auto_available: true,
            }),
            PERMISSION_MODE_YOLO
        );
        assert_eq!(
            permission_mode_current(PermissionModeView {
                yolo: false,
                auto: true,
                auto_available: false,
            }),
            PERMISSION_MODE_AUTO
        );
        let hidden = permission_mode_option(PermissionModeView {
            yolo: false,
            auto: false,
            auto_available: false,
        });
        let v = serde_json::to_value(&hidden).expect("serialize");
        assert_eq!(v.get("category").and_then(|c| c.as_str()), Some("mode"));
        assert_eq!(
            v.get("currentValue").and_then(|c| c.as_str()),
            Some(PERMISSION_MODE_ASK)
        );
        let ids: Vec<_> = v
            .get("options")
            .and_then(|o| o.as_array())
            .expect("flat options array")
            .iter()
            .filter_map(|option| option.get("value").and_then(|value| value.as_str()))
            .collect();
        assert_eq!(ids, vec![PERMISSION_MODE_ASK, PERMISSION_MODE_YOLO]);
    }

    #[test]
    fn acp_config_options_effort_current_preserves_unlisted_value() {
        let models = [model("grok-4.5", "Grok 4.5")];
        let efforts = [ReasoningEffortOption {
            id: "high".to_string(),
            value: ReasoningEffort::High,
            label: "High".to_string(),
            description: None,
            default: false,
        }];
        let options = build_acp_config_options(
            &models,
            &acp::ModelId::from("grok-4.5"),
            &efforts,
            Some(ReasoningEffort::Low),
            PermissionModeView {
                yolo: true,
                auto: false,
                auto_available: false,
            },
        );
        let effort = options
            .iter()
            .find(|o| o.id.0.as_ref() == CONFIG_ID_REASONING_EFFORT)
            .expect("effort selector present when the model supports effort");
        match &effort.kind {
            acp::SessionConfigKind::Select(select) => {
                assert_eq!(select.current_value.0.as_ref(), "low");
            }
            _ => panic!("effort must be a select"),
        }
    }

    #[test]
    fn acp_config_options_model_current_preserves_unlisted_value() {
        let models = [
            model("grok-build", "Grok Build"),
            model("grok-4.5", "Grok 4.5"),
        ];
        let options = build_acp_config_options(
            &models,
            &acp::ModelId::from("stale-model"),
            &[],
            None,
            PermissionModeView {
                yolo: false,
                auto: false,
                auto_available: false,
            },
        );
        let model = options
            .iter()
            .find(|o| o.id.0.as_ref() == CONFIG_ID_MODEL)
            .expect("model selector present");
        match &model.kind {
            acp::SessionConfigKind::Select(select) => {
                assert_eq!(select.current_value.0.as_ref(), "stale-model");
            }
            _ => panic!("model must be a select"),
        }
    }
}
