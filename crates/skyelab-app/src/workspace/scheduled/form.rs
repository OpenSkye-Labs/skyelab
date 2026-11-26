//! The task editor's values and the rules a save checks: what it schedules, where, as whom, and how.

use skyelab_agent::Mode;
use skyelab_core::AccountsConfig;
use skyelab_core::schedule::{Schedule, check_mode};
use skyelab_store::Project;
use skyelab_store::scheduled::ScheduledTask;

/// The schedule picker's kinds; each takes one typed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Kind {
    Minutes,
    Hours,
    #[default]
    Daily,
    Weekdays,
    Cron,
}

impl Kind {
    pub(crate) const ALL: [Kind; 5] = [
        Kind::Minutes,
        Kind::Hours,
        Kind::Daily,
        Kind::Weekdays,
        Kind::Cron,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Minutes => "Every N minutes",
            Kind::Hours => "Every N hours",
            Kind::Daily => "Daily at",
            Kind::Weekdays => "Weekdays at",
            Kind::Cron => "Cron",
        }
    }

    /// What the value field asks for.
    pub(crate) fn hint(self) -> &'static str {
        match self {
            Kind::Minutes => "30",
            Kind::Hours => "2",
            Kind::Daily | Kind::Weekdays => "09:00",
            Kind::Cron => "0 9 * * 1-5",
        }
    }

    fn spec(self, value: &str) -> String {
        let v = value.trim();
        match self {
            Kind::Minutes => format!("every {v}m"),
            Kind::Hours => format!("every {v}h"),
            Kind::Daily => format!("daily {v}"),
            Kind::Weekdays => format!("weekdays {v}"),
            Kind::Cron => format!("cron {v}"),
        }
    }

    /// A stored spec back into the picker's kind and value.
    pub(crate) fn of(spec: &str) -> (Kind, String) {
        match spec.parse::<Schedule>() {
            Ok(Schedule::Every { minutes }) if minutes % 60 == 0 => {
                (Kind::Hours, (minutes / 60).to_string())
            }
            Ok(Schedule::Every { minutes }) => (Kind::Minutes, minutes.to_string()),
            Ok(Schedule::Daily { hour, minute }) => (Kind::Daily, format!("{hour:02}:{minute:02}")),
            Ok(Schedule::Weekdays { hour, minute }) => {
                (Kind::Weekdays, format!("{hour:02}:{minute:02}"))
            }
            Ok(Schedule::Cron(p)) => (Kind::Cron, p),
            Err(_) => (Kind::Daily, "09:00".into()),
        }
    }
}

/// What the editor holds besides its text fields' entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Draft {
    pub id: i64,
    pub title: String,
    pub prompt: String,
    pub kind: Kind,
    pub value: String,
    pub project: Option<i64>,
    pub worktree: bool,
    pub mode: Mode,
    pub bypass_confirmed: bool,
    pub model: String,
    pub enabled: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            id: 0,
            title: String::new(),
            prompt: String::new(),
            kind: Kind::Daily,
            value: "09:00".into(),
            project: None,
            worktree: false,
            mode: Mode::Default,
            bypass_confirmed: false,
            model: String::new(),
            enabled: true,
        }
    }
}

impl Draft {
    pub(crate) fn of(t: &ScheduledTask) -> Self {
        let (kind, value) = Kind::of(&t.schedule);
        Self {
            id: t.id,
            title: t.title.clone(),
            prompt: t.prompt.clone(),
            kind,
            value,
            project: Some(t.project_id),
            worktree: t.worktree,
            mode: Mode::parse(&t.permission_mode),
            bypass_confirmed: t.bypass_confirmed,
            model: t.model.clone().unwrap_or_default(),
            enabled: t.enabled,
        }
    }

    /// The task a save writes, or why it can't. The account is the folder's now: saving under an unbound folder is
    /// refused, never given a default. A save counts the next run on from `now`.
    pub(crate) fn to_task(
        &self,
        projects: &[Project],
        config: &AccountsConfig,
        now: i64,
    ) -> Result<ScheduledTask, String> {
        let prompt = self.prompt.trim();
        if prompt.is_empty() {
            return Err("Write the prompt each run sends.".into());
        }
        if let Some(why) = skyelab_core::prompt::over_limit(prompt) {
            return Err(why);
        }
        let project = self
            .project
            .and_then(|id| projects.iter().find(|p| p.id == id))
            .ok_or("Pick the project it runs in.")?;
        let spec = self.kind.spec(&self.value);
        let schedule: Schedule = spec.parse()?;
        check_mode(self.mode.as_str(), self.bypass_confirmed)?;
        let account = config
            .resolve(&project.path)
            .ok_or_else(|| format!("{} isn't bound to an account. Bind it first.", project.name))?;
        let model = self.model.trim();
        let title = match self.title.trim() {
            "" => prompt
                .lines()
                .next()
                .unwrap_or(prompt)
                .chars()
                .take(60)
                .collect(),
            t => t.to_owned(),
        };
        Ok(ScheduledTask {
            id: self.id,
            project_id: project.id,
            title,
            prompt: prompt.to_owned(),
            schedule: schedule.to_string(),
            worktree: self.worktree,
            permission_mode: self.mode.as_str().to_owned(),
            bypass_confirmed: self.mode == Mode::Bypass && self.bypass_confirmed,
            model: (!model.is_empty()).then(|| model.to_owned()),
            effort: None,
            account_id: account.id.clone(),
            enabled: self.enabled,
            next_run_at: super::next_run(&schedule.to_string(), now),
            ..Default::default()
        })
    }
}
