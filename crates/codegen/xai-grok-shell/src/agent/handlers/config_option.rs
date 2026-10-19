use agent_client_protocol::{self as acp};

use crate::agent::handlers::model_switch::{self, ConfigNotice};
use crate::agent::mvp_agent::MvpAgent;
use crate::agent::session_config::{
    CONFIG_ID_MODEL, CONFIG_ID_PERMISSION_MODE, CONFIG_ID_REASONING_EFFORT, PERMISSION_MODE_ASK,
    PERMISSION_MODE_AUTO, PERMISSION_MODE_YOLO,
};
use crate::session::SessionCommand;

pub(crate) async fn apply(
    agent: &MvpAgent,
    args: acp::SetSessionConfigOptionRequest,
) -> Result<acp::SetSessionConfigOptionResponse, acp::Error> {
    let acp::SetSessionConfigOptionRequest {
        session_id,
        config_id,
        value,
        ..
    } = args;

    match config_id.0.as_ref() {
        CONFIG_ID_MODEL => {
            let value_id = value.as_value_id().ok_or_else(|| {
                acp::Error::invalid_params().data("model requires a string value")
            })?;
            let request = acp::SetSessionModelRequest::new(
                session_id.clone(),
                acp::ModelId::new(value_id.0.clone()),
            );
            agent.set_model_gated(request).await?;
        }
        CONFIG_ID_REASONING_EFFORT => {
            let value_id = value.as_value_id().ok_or_else(|| {
                acp::Error::invalid_params().data("reasoning_effort requires a string value")
            })?;
            // The selector is resolved inside `apply_reasoning_effort` under the config lock, so
            // it is validated against the model the effort will actually run on.
            model_switch::apply_reasoning_effort(
                agent,
                session_id.clone(),
                value_id.0.as_ref(),
                ConfigNotice::Send,
            )
            .await?;
        }
        CONFIG_ID_PERMISSION_MODE => {
            let value_id = value.as_value_id().ok_or_else(|| {
                acp::Error::invalid_params().data("permission_mode requires a string value")
            })?;
            apply_permission_mode(agent, &session_id, value_id.0.as_ref())?;
        }
        other => {
            return Err(
                acp::Error::invalid_params().data(format!("unknown config option: {other}"))
            );
        }
    }

    let options = agent.acp_config_options_for_session(&session_id).await;
    Ok(acp::SetSessionConfigOptionResponse::new(options))
}

/// Switch the resident session between ask, auto, and always-approve.
///
/// The permission-handle write is synchronous so the config-option response sees the new
/// posture. The session command repeats it so auto mode wires (or drops) the classifier and
/// so a YOLO toggle still emits `YoloToggled`. `acceptEdits` is not accepted: it is a startup
/// rule, not a live mode.
fn apply_permission_mode(
    agent: &MvpAgent,
    session_id: &acp::SessionId,
    value: &str,
) -> Result<(), acp::Error> {
    let applied = agent.with_resident_mut(session_id, |handle| match value {
        PERMISSION_MODE_ASK => {
            handle.permission_handle.set_yolo_mode(false);
            handle.permission_handle.set_auto_mode(false);
            let _ = handle
                .cmd_tx
                .send(SessionCommand::SetYoloMode { enabled: false });
            let _ = handle
                .cmd_tx
                .send(SessionCommand::SetAutoMode { enabled: false });
            handle.yolo_mode = false;
            Ok(())
        }
        PERMISSION_MODE_AUTO => {
            if !crate::util::config::auto_permission_mode_enabled_from_disk() {
                return Err(acp::Error::invalid_params()
                    .data("auto permission mode is disabled"));
            }
            handle.permission_handle.set_auto_mode(true);
            let _ = handle
                .cmd_tx
                .send(SessionCommand::SetAutoMode { enabled: true });
            handle.yolo_mode = false;
            Ok(())
        }
        PERMISSION_MODE_YOLO => {
            handle.permission_handle.set_yolo_mode(true);
            if !handle.permission_handle.is_yolo_mode() {
                return Err(acp::Error::invalid_params()
                    .data("always-approve is disabled by managed policy"));
            }
            let _ = handle
                .cmd_tx
                .send(SessionCommand::SetYoloMode { enabled: true });
            handle.yolo_mode = true;
            Ok(())
        }
        other => Err(acp::Error::invalid_params()
            .data(format!("unknown permission mode: {other}"))),
    });
    applied.unwrap_or_else(|| {
        Err(acp::Error::invalid_params().data("session is not resident"))
    })
}
