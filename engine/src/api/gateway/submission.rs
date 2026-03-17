use sdk::TaskSource;
use tracing::{info, warn};
use uuid::Uuid;

use super::Gateway;

impl Gateway {
    pub async fn submit_cli(&self, input: &str, password: Option<&str>) -> anyhow::Result<String> {
        if let Some(required_password) = self.config.cli_password.as_deref() {
            match password {
                Some(provided) if provided == required_password => {}
                Some(_) => return Err(anyhow::anyhow!("Invalid CLI password")),
                None => return Err(anyhow::anyhow!("CLI password required but not provided")),
            }
        }

        self.submit_task(input, TaskSource::Cli, None, None, None).await
    }

    pub async fn submit_telegram(
        &self,
        input: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<String> {
        self.submit_task(input, TaskSource::Telegram(String::new()), session_id, None, None)
            .await
    }

    pub async fn submit_webui(
        &self,
        input: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<String> {
        self.submit_task(input, TaskSource::WebUI, session_id, None, None)
            .await
    }

    pub async fn submit_remote(
        &self,
        input: &str,
        session_id: Option<&str>,
        workspace: Option<&str>,
        team_id: Option<&str>,
    ) -> anyhow::Result<String> {
        self.submit_task(
            input,
            TaskSource::Remote(String::new()),
            session_id,
            workspace,
            team_id,
        )
        .await
    }

    async fn submit_task(
        &self,
        input: &str,
        source: TaskSource,
        session_id: Option<&str>,
        workspace: Option<&str>,
        team_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let task_id = Uuid::new_v4();
        let repo = self.db.pending_tasks();

        let safe_input = if let Some(warning) = self.injection_detector.scan(input) {
            warn!(
                task_id = %task_id,
                source = ?source,
                pattern = %warning.matched_pattern,
                position = warning.position,
                "Injection attempt detected at gateway entry (Layer 1)"
            );
            self.injection_detector.sanitize(input)
        } else {
            input.to_string()
        };

        let dispatch = self.dispatch_brain.classify(&safe_input);
        info!(
            task_id = %task_id,
            domain = ?dispatch.domain,
            complexity = ?dispatch.complexity,
            sensitive = dispatch.sensitive,
            "Task classified by dispatch brain"
        );

        repo.create_task_with_dispatch(
            &task_id.to_string(),
            &safe_input,
            source.clone(),
            &dispatch.domain.to_string().to_lowercase(),
            &format!("{:?}", dispatch.complexity).to_lowercase(),
            dispatch.sensitive,
            session_id,
            workspace,
            team_id,
        )
        .await?;

        info!(task_id = %task_id, "Submitted task from {:?}", source);
        Ok(task_id.to_string())
    }
}
