use anyhow::{bail, Result};

use crate::cli::{
    ApprovalModeAction, ApprovalModeArg, ApprovalRuleActionArg, ApprovalRuleCommand,
    ApprovalsAction,
};
use crate::config::{ApprovalMode, Config};
use crate::security::approvals::{self, ApprovalRule, ApprovalRuleAction, ApprovalRulesFile};

pub fn handle(action: ApprovalsAction, config: &mut Config) -> Result<()> {
    match action {
        ApprovalsAction::Mode { action } => handle_mode(action, config),
        ApprovalsAction::Rules { action } => handle_rules(action, config),
    }
}

fn handle_mode(action: ApprovalModeAction, config: &mut Config) -> Result<()> {
    match action {
        ApprovalModeAction::Show => {
            println!("approval_mode: {}", config.approvals.mode.as_str());
        }
        ApprovalModeAction::Set { mode } => {
            config.approvals.mode = match mode {
                ApprovalModeArg::Default => ApprovalMode::Default,
                ApprovalModeArg::Allowlist => ApprovalMode::Allowlist,
                ApprovalModeArg::Open => ApprovalMode::Open,
                ApprovalModeArg::Assisted => ApprovalMode::Assisted,
            };
            config.save()?;
            println!("approval_mode: {}", config.approvals.mode.as_str());
        }
    }
    Ok(())
}

fn handle_rules(action: ApprovalRuleCommand, config: &Config) -> Result<()> {
    match action {
        ApprovalRuleCommand::List => list_rules(config),
        ApprovalRuleCommand::Add {
            id,
            action,
            tool,
            commands,
            paths,
            nodes,
            channels,
            risk_tier,
            effect,
        } => add_rule(
            config,
            ApprovalRule {
                id,
                action: match action {
                    ApprovalRuleActionArg::Allow => ApprovalRuleAction::Allow,
                    ApprovalRuleActionArg::RequireApproval => ApprovalRuleAction::RequireApproval,
                },
                tool,
                commands,
                paths,
                nodes,
                channels,
                risk_tier,
                effect,
            },
        ),
        ApprovalRuleCommand::Remove { id } => remove_rule(config, &id),
    }
}

fn list_rules(config: &Config) -> Result<()> {
    let path = approvals::rules_path(config)?;
    let file = approvals::load_rules(config)?;
    println!("approval_rules_path: {}", path.display());
    if file.rules.is_empty() {
        println!("rules: (none)");
        return Ok(());
    }
    for rule in file.rules {
        println!(
            "- {} action={:?} tool={} risk_tier={} effect={}",
            rule.id,
            rule.action,
            rule.tool.as_deref().unwrap_or("*"),
            rule.risk_tier
                .map(|value| value.to_string())
                .unwrap_or_else(|| "*".to_string()),
            rule.effect.as_deref().unwrap_or("*"),
        );
    }
    Ok(())
}

fn add_rule(config: &Config, rule: ApprovalRule) -> Result<()> {
    if rule.id.trim().is_empty() {
        bail!("Rule id cannot be empty");
    }
    let file = approvals::add_rule(config, rule)?;
    print_rule_count(&file);
    Ok(())
}

fn remove_rule(config: &Config, id: &str) -> Result<()> {
    if approvals::remove_rule(config, id)? {
        let file = approvals::load_rules(config)?;
        print_rule_count(&file);
        Ok(())
    } else {
        bail!("Approval rule '{}' was not found", id)
    }
}

fn print_rule_count(file: &ApprovalRulesFile) {
    println!("approval_rules: {}", file.rules.len());
}
