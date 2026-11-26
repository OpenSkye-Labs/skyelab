//! Bots' GitHub triggers: every `POLL_EVERY_MS`, per account and project, one `gh pr list` / `gh issue list` (through
//! that account's own gh) serves every bot there. The folder's binding is checked before each poll: a bot whose
//! folder no longer resolves to its saved account is paused and nothing is listed. A hit is queued, with its item
//! framed as untrusted text, for the scheduler's tick to start like any other run. The rules live in
//! `skyelab_core::bots`.

use super::super::gh_pages::{Filter, Listing, read_group};
use super::super::{SharedStore, lock, windows};
use super::runs::pause;
use super::{Scheduler, now_ms};
use gpui::{App, AppContext as _};
use skyelab_core::AccountsConfig;
use skyelab_core::bots::{
    self, Item, POLL_EVERY_MS, QUEUE_CAP, Trigger, check_trigger_mode, framed_prompt, hits,
    item_url, limited_hours_in_a_row, over_hour_limit, parse_triggers,
};
use skyelab_core::schedule::run_account;
use skyelab_git::{Checks, IssueFilter, PrFilter};
use skyelab_store::scheduled::{NewBotRun, ScheduledTask};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// (account, project): one poll serves every bot of that project under that account.
type Key = (String, i64);

#[derive(Default)]
pub(crate) struct Polls {
    last: HashMap<Key, i64>,
    inflight: HashSet<Key>,
    /// Why a project's last poll failed, wholly or for one list (no GitHub remote, gh not signed in), for the Bots page.
    pub(crate) errors: HashMap<i64, String>,
}

fn accounts_path(cx: &App) -> Option<PathBuf> {
    windows::hub_workspace(cx)
        .and_then(|(_, ws)| ws.read(cx).accounts_path.clone())
        .or_else(|| {
            cx.try_global::<super::super::AccountsFile>()
                .map(|f| f.0.clone())
        })
}

/// The list `trigger` reads for `bot`: its PRs or issues, anyone's or only the account's own.
fn list_for(bot: &ScheduledTask, trigger: Trigger) -> Filter {
    let mine = bot.author_filter == "me";
    match trigger.lists_prs() {
        true => Filter::Pr(if mine { PrFilter::Mine } else { PrFilter::All }),
        false => Filter::Issue(if mine {
            IssueFilter::Mine
        } else {
            IssueFilter::All
        }),
    }
}

/// Starts the polls that are due. Called from every tick.
pub(super) fn poll(store: &SharedStore, now: i64, cx: &mut App) {
    // Read now, not the tick's list: a run refused earlier in this tick may have paused one already.
    let tasks = match lock(store).and_then(|s| s.scheduled_tasks()) {
        Ok(t) => t,
        Err(e) => return eprintln!("bots: {e:#}"),
    };
    let bots: Vec<_> = tasks
        .iter()
        .filter(|t| t.enabled && !t.triggers.is_empty())
        .collect();
    if bots.is_empty() {
        return;
    }
    let path = accounts_path(cx);
    // The file as it is now; one that can't be read binds nothing, so every bot pauses rather than polls.
    let config = path
        .as_deref()
        .and_then(|p| AccountsConfig::load(p).ok())
        .unwrap_or_default();
    let projects = match lock(store).and_then(|s| s.list_projects()) {
        Ok(p) => p,
        Err(e) => return eprintln!("bots: {e:#}"),
    };
    let mut groups: HashMap<Key, (PathBuf, Vec<Filter>)> = HashMap::new();
    for bot in bots {
        let Some(project) = projects.iter().find(|p| p.id == bot.project_id) else {
            let why = "its project was removed from SkyeLab";
            super::runs::add_run(store, bot.id, now, "poll", "paused", Some(why));
            pause(store, bot, now, why, cx);
            continue;
        };
        let checked = run_account(&config, &project.path, &bot.account_id)
            .and_then(|a| check_trigger_mode(&bot.permission_mode, true, a.provider));
        if let Err(why) = checked {
            super::runs::add_run(store, bot.id, now, "poll", "paused", Some(&why));
            pause(store, bot, now, &why, cx);
            continue;
        }
        let entry = groups
            .entry((bot.account_id.clone(), project.id))
            .or_insert_with(|| (project.path.clone(), Vec::new()));
        for f in parse_triggers(&bot.triggers)
            .into_iter()
            .map(|t| list_for(bot, t))
        {
            if !entry.1.contains(&f) {
                entry.1.push(f);
            }
        }
    }
    let gh = crate::workspace::gh_bin(cx);
    for (key, (dir, filters)) in groups {
        let polls = &mut cx.default_global::<Scheduler>().polls;
        if polls.inflight.contains(&key)
            || polls
                .last
                .get(&key)
                .is_some_and(|&t| now - t < POLL_EVERY_MS)
        {
            continue;
        }
        polls.inflight.insert(key.clone());
        polls.last.insert(key.clone(), now);
        let (path, gh, store) = (path.clone(), gh.clone(), store.clone());
        let listed = cx.background_spawn(async move {
            // Read again off the main thread: `read_group` resolves the folder and lists only for its account.
            let config = path
                .as_deref()
                .and_then(|p| AccountsConfig::load(p).ok())
                .unwrap_or_default();
            let repo = skyelab_git::repo_slug(&dir).map_err(|e| e.to_string());
            let lists: Vec<_> = filters
                .into_iter()
                .map(|f| (f, read_group(&config, &dir, &gh, f)))
                .collect();
            (repo, lists)
        });
        cx.spawn(async move |cx| {
            let (repo, lists) = listed.await;
            cx.update(|cx| apply(&store, key, repo, lists, cx));
        })
        .detach();
    }
}

fn items(listing: &Listing) -> Result<Vec<Item>, String> {
    Ok(match listing {
        Listing::Prs(rows) => rows
            .iter()
            .map(|d| Item {
                number: d.row.number,
                title: d.row.title.clone(),
                author: d.row.author.clone(),
                body: d.row.body.clone(),
                draft: d.row.draft,
                head_branch: d.row.head_branch.clone(),
                head_sha: d.row.head.clone(),
                failing: d.row.checks == Checks::Failing,
                ..Default::default()
            })
            .collect(),
        Listing::Issues(rows) => rows
            .iter()
            .map(|d| Item {
                number: d.row.number,
                title: d.row.title.clone(),
                author: d.row.author.clone(),
                labels: d.row.labels.clone(),
                body: d.row.body.clone(),
                ..Default::default()
            })
            .collect(),
        Listing::SignIn => return Err("its account's GitHub CLI isn't signed in".into()),
        Listing::NoGitHub(why) => return Err(why.to_string()),
        Listing::Failed(why) => return Err(why.to_string()),
        Listing::Unbound | Listing::Loading => return Err("its folder isn't bound".into()),
    })
}

/// A poll's lists: queue each bot's new hits, or only record what exists on its first poll.
fn apply(
    store: &SharedStore,
    key: Key,
    repo: Result<String, String>,
    lists: Vec<(Filter, super::super::gh_pages::Group)>,
    cx: &mut App,
) {
    let now = now_ms(cx);
    let polls = &mut cx.default_global::<Scheduler>().polls;
    polls.inflight.remove(&key);
    let (account, project) = key;
    let mut listed = HashMap::new();
    let mut error = repo.as_ref().err().cloned();
    for (filter, group) in lists {
        // Rebound while listing: these rows are another account's. The next poll pauses the bot.
        if group.account.as_ref().map(|a| &a.0) != Some(&account) {
            return;
        }
        match items(&group.listing) {
            Ok(items) => drop(listed.insert(filter, items)),
            Err(why) => error = Some(why),
        }
    }
    match &error {
        Some(why) => drop(polls.errors.insert(project, why.clone())),
        None => drop(polls.errors.remove(&project)),
    }
    let Ok(repo) = repo else { return };
    // Read again: a bot saved, paused or rebound while the list ran.
    let bots = match lock(store).and_then(|s| s.scheduled_tasks()) {
        Ok(t) => t,
        Err(e) => return eprintln!("bots: {e:#}"),
    };
    for bot in bots.iter().filter(|b| {
        b.enabled && !b.triggers.is_empty() && b.project_id == project && b.account_id == account
    }) {
        if let Err(e) = fire(store, bot, &repo, &listed, now, cx) {
            eprintln!("bot {}: {e:#}", bot.id);
        }
    }
    cx.default_global::<Scheduler>().changes += 1;
}

fn fire(
    store: &SharedStore,
    bot: &ScheduledTask,
    repo: &str,
    listed: &HashMap<Filter, Vec<Item>>,
    now: i64,
    cx: &mut App,
) -> anyhow::Result<()> {
    let (seen, branches) = {
        let s = lock(store)?;
        (s.bot_seen(bot.id)?, s.bot_branches(bot.project_id)?)
    };
    let mut new = Vec::new();
    let mut eligible = Vec::new();
    let mut all_listed = true;
    for trigger in parse_triggers(&bot.triggers) {
        let Some(items) = listed.get(&list_for(bot, trigger)) else {
            // That list failed: a primed bot catches up on a later poll; an unprimed one stays unprimed.
            all_listed = false;
            continue;
        };
        for item in hits(trigger, items, &HashSet::new(), &branches) {
            let key = bots::key(trigger, item);
            if !seen.contains(&key) {
                new.push((trigger, item));
            }
            eligible.push(key);
        }
    }
    let mut limited_now = false;
    if bot.gh_primed {
        for (trigger, item) in new {
            limited_now |= queue(store, bot, repo, trigger, item, now)?;
        }
    }
    let s = lock(store)?;
    s.mark_bot_seen(bot.id, &eligible, now)?;
    if !bot.gh_primed && all_listed {
        s.set_gh_primed(bot.id)?;
    }
    let runs = s.bot_runs(bot.id, 200)?;
    drop(s);
    // Only limits since its last pause count: turned back on, a bot starts over.
    let limited: Vec<i64> = runs
        .iter()
        .take_while(|r| r.status != "paused")
        .filter(|r| r.status == "rate_limited")
        .map(|r| r.at)
        .collect();
    if limited_now && limited_hours_in_a_row(&limited, now) {
        let why = "it hit its hourly limit three hours in a row";
        super::runs::add_run(store, bot.id, now, "poll", "paused", Some(why));
        pause(store, bot, now, why, cx);
    }
    Ok(())
}

/// Queues one hit, or records it as rate limited (over the hourly cap, or the queue is full): returns which.
fn queue(
    store: &SharedStore,
    bot: &ScheduledTask,
    repo: &str,
    trigger: Trigger,
    item: &Item,
    now: i64,
) -> anyhow::Result<bool> {
    let s = lock(store)?;
    let runs = s.bot_runs(bot.id, 200)?;
    let github = |t: &str| Trigger::parse(t).is_some();
    let started: Vec<i64> = runs
        .iter()
        .filter(|r| github(&r.trigger) && matches!(r.status.as_str(), "queued" | "started"))
        .map(|r| r.at)
        .collect();
    let waiting = runs.iter().filter(|r| r.status == "queued").count();
    let url = item_url(repo, trigger, item.number);
    let limited = over_hour_limit(&started, now, bot.max_runs_per_hour) || waiting >= QUEUE_CAP;
    let prompt = framed_prompt(&bot.prompt, trigger, &url, item);
    s.add_bot_run(&NewBotRun {
        task: bot.id,
        at: now,
        trigger: trigger.as_str(),
        item_url: Some(&url),
        item_prompt: (!limited).then_some(prompt.as_str()),
        status: if limited { "rate_limited" } else { "queued" },
        error: None,
    })?;
    Ok(limited)
}

/// Whether `dir` can take GitHub triggers: bound, a github.com `origin`, and its account's gh signed in. Blocking.
pub(super) fn github_ready(
    accounts: Option<&std::path::Path>,
    dir: &std::path::Path,
) -> Result<(), String> {
    let config = accounts
        .and_then(|p| AccountsConfig::load(p).ok())
        .unwrap_or_default();
    let account = config
        .resolve(dir)
        .ok_or("Its folder isn't bound to an account.")?;
    skyelab_git::repo_slug(dir).map_err(|e| format!("No GitHub repo: {e}."))?;
    let signed_in = account.gh_config_dir.is_some()
        && config
            .github_config_dir(dir)
            .is_ok_and(|d| skyelab_git::gh_logged_in(&d));
    match signed_in {
        true => Ok(()),
        false => Err(format!("{}'s GitHub CLI isn't signed in.", account.label)),
    }
}
